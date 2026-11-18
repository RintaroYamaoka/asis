//! 部屋。repo とメンバーと役割をまとめた、共有の定義。DB(Supabase)に置く。
//!
//! DB に入れるのは部屋の定義だけ。issue・PR・コードは、今までどおり各自の PC が GitHub から読む。
//! ログインは本人確認だけに使う。repo を読む鍵は DB に渡さない。
use crate::config::{self, Db, Project};
use asis_core::facts::AUTHOR;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// ログインの戻りを PC の中で受ける場所。DB の側の許可の一覧と同じ値にする。
const CALLBACK: &str = "127.0.0.1:53682";

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Member {
    pub github_id: i64,
    pub login: String,
    pub is_admin: bool,
    pub review: bool,
    pub merge: bool,
    pub deploy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub repos: Vec<String>,
    /// メンバーでない人の分も出す repo
    pub everyone: Vec<String>,
    pub members: Vec<Member>,
    pub merge_by_author: bool,
    pub deploy_by_author: bool,
    pub ignore_checks: Vec<String>,
}

#[derive(Deserialize)]
struct RepoRow {
    repo: String,
    everyone: bool,
}

#[derive(Deserialize)]
struct RawRoom {
    id: String,
    name: String,
    merge_by_author: bool,
    deploy_by_author: bool,
    ignore_checks: Vec<String>,
    room_repos: Vec<RepoRow>,
    room_members: Vec<Member>,
}

/// 部屋の一覧を聞くときの問い合わせ
const ROOMS: &str = "/rest/v1/rooms?select=id,name,merge_by_author,deploy_by_author,ignore_checks,\
room_repos(repo,everyone),room_members(github_id,login,is_admin,review,merge,deploy)&order=created_at";

pub fn parse_rooms(text: &str) -> Result<Vec<Room>, String> {
    let raw: Vec<RawRoom> =
        serde_json::from_str(text).map_err(|e| format!("部屋の一覧を読めません: {e}"))?;
    Ok(raw
        .into_iter()
        .map(|r| {
            let everyone = r
                .room_repos
                .iter()
                .filter(|x| x.everyone)
                .map(|x| x.repo.clone())
                .collect();
            let mut repos: Vec<String> = r.room_repos.into_iter().map(|x| x.repo).collect();
            repos.sort();
            let mut members = r.room_members;
            members.sort_by_key(|m| m.login.to_lowercase());
            Room {
                id: r.id,
                name: r.name,
                repos,
                everyone,
                members,
                merge_by_author: r.merge_by_author,
                deploy_by_author: r.deploy_by_author,
                ignore_checks: r.ignore_checks,
            }
        })
        .collect())
}

impl Room {
    fn role(&self, has: impl Fn(&Member) -> bool, by_author: bool) -> Vec<String> {
        let mut names: Vec<String> = self
            .members
            .iter()
            .filter(|m| has(m))
            .map(|m| m.login.clone())
            .collect();
        if by_author {
            names.push(AUTHOR.to_string());
        }
        names
    }

    /// 状態と番を決めるための定義にする。
    pub fn project(&self) -> Project {
        Project {
            name: self.name.clone(),
            repos: self.repos.clone(),
            members: self.members.iter().map(|m| m.login.clone()).collect(),
            review: self.role(|m| m.review, false),
            merge: self.role(|m| m.merge, self.merge_by_author),
            deploy: self.role(|m| m.deploy, self.deploy_by_author),
            ignore_checks: self.ignore_checks.clone(),
            room: Some(self.id.clone()),
        }
    }

    /// この部屋の、その repo で出す人。空なら全員。
    /// 既定はメンバーの分だけ。「全員」にした repo は、メンバーでない人の PR も出す。
    pub fn people(&self, repo: &str) -> Vec<String> {
        if self.everyone.iter().any(|r| r == repo) {
            return Vec::new();
        }
        self.members.iter().map(|m| m.login.clone()).collect()
    }
}

/// 名前で部屋を選ぶ。同じ名前が 2 つあるときは選べない。
pub fn find<'a>(rooms: &'a [Room], name: &str) -> Result<&'a Room, String> {
    let hits: Vec<&Room> = rooms.iter().filter(|r| r.name == name).collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("`{name}` という部屋はありません")),
        _ => Err(format!("`{name}` という部屋が 2 つ以上あります")),
    }
}

fn base64url(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// ログインの途中で横取りされても使えないようにするための合言葉と、その印。
/// 印だけを先に DB へ送り、合言葉は最後の受け取りのときに出す。
fn challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

fn new_verifier() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("乱数を作れません: {e}"))?;
    Ok(base64url(&bytes))
}

fn percent_decode(s: &str) -> String {
    let (bytes, mut out, mut i) = (s.as_bytes(), Vec::new(), 0);
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 2;
            }
            (b'+', _) => out.push(b' '),
            (b, _) => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// ブラウザから戻ってきた要求の 1 行目から、受け取りの番号を取り出す。
fn parse_callback(request_line: &str) -> Result<String, String> {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let query = target
        .strip_prefix("/callback")
        .and_then(|rest| rest.strip_prefix('?'))
        .ok_or("ログインの戻りではない要求が来ました")?;
    let get = |key: &str| {
        query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == key)
            .map(|(_, v)| percent_decode(v))
    };
    match (
        get("code"),
        get("error_description").or_else(|| get("error")),
    ) {
        (Some(code), _) if !code.is_empty() => Ok(code),
        (_, Some(error)) => Err(format!("ログインできませんでした: {error}")),
        _ => Err("ログインの戻りに、受け取りの番号がありません".into()),
    }
}

/// ログインの記録。この PC の中にだけ置く。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Session {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
}

impl Session {
    /// 切れているか、もうすぐ切れるか
    fn is_stale(&self, now: i64) -> bool {
        self.expires_at < now + 60
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn session_path() -> Result<PathBuf, String> {
    config::path()
        .map(|p| p.with_file_name("session.json"))
        .ok_or_else(|| "設定の置き場が分かりません(HOME がありません)".to_string())
}

fn save_session(session: &Session) -> Result<(), String> {
    let path = session_path()?;
    let fail = |e: std::io::Error| format!("{}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let text = serde_json::to_string(session).map_err(|e| e.to_string())?;
    options
        .open(&path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .map_err(fail)
}

fn load_session() -> Result<Option<Session>, String> {
    let path = session_path()?;
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(serde_json::from_str(&text).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// DB が返した失敗の理由を、人が読める 1 行にする。
fn explain(status: u16, body: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let text = ["message", "msg", "error_description", "error"]
        .iter()
        .find_map(|k| v.get(k).and_then(|m| m.as_str()))
        .unwrap_or(body.trim());
    match v.get("code").and_then(|c| c.as_str()) {
        Some("42501") if !text.contains("許可") => {
            "その操作は、部屋の管理者だけができます".to_string()
        }
        Some("23505") => "もう入っています".to_string(),
        _ => format!("{text} (HTTP {status})"),
    }
}

struct Http {
    db: Db,
    agent: ureq::Agent,
}

impl Http {
    fn new(db: &Db) -> Http {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(20)))
            .build();
        Http {
            db: db.clone(),
            agent: config.into(),
        }
    }

    /// `token` があればログインした人として、無ければ公開用の鍵だけで聞く。
    fn send(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&str>,
        prefer: Option<&str>,
    ) -> Result<String, String> {
        let url = format!("{}{path}", self.db.url);
        let fail = |e: ureq::Error| format!("DB につながりません: {e}");
        let bearer = format!("Bearer {}", token.unwrap_or(&self.db.key));
        let mut response = match (method, body) {
            ("GET", _) => self
                .agent
                .get(&url)
                .header("apikey", &self.db.key)
                .header("authorization", &bearer)
                .call(),
            ("DELETE", _) => self
                .agent
                .delete(&url)
                .header("apikey", &self.db.key)
                .header("authorization", &bearer)
                .header("prefer", prefer.unwrap_or(""))
                .call(),
            (m, body) => {
                let request = if m == "PATCH" {
                    self.agent.patch(&url)
                } else {
                    self.agent.post(&url)
                };
                request
                    .header("apikey", &self.db.key)
                    .header("authorization", &bearer)
                    .header("content-type", "application/json")
                    .header("prefer", prefer.unwrap_or(""))
                    .send(body.unwrap_or("{}"))
            }
        }
        .map_err(fail)?;
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().map_err(fail)?;
        if (200..300).contains(&status) {
            Ok(text)
        } else {
            Err(explain(status, &text))
        }
    }

    fn token(&self, grant: &str, body: &serde_json::Value) -> Result<Session, String> {
        let text = self.send(
            "POST",
            &format!("/auth/v1/token?grant_type={grant}"),
            None,
            Some(&body.to_string()),
            None,
        )?;
        let mut session: Session =
            serde_json::from_str(&text).map_err(|e| format!("ログインの返事を読めません: {e}"))?;
        if session.expires_at == 0 {
            session.expires_at = now() + 3600;
        }
        Ok(session)
    }
}

/// ログインした人として DB と話す口
pub struct Client {
    http: Http,
    token: String,
}

impl Client {
    /// ログインの記録を読む。切れていれば更新する。ログインしていなければ `None`。
    pub fn open(db: &Db) -> Result<Option<Client>, String> {
        let Some(mut session) = load_session()? else {
            return Ok(None);
        };
        let http = Http::new(db);
        if session.is_stale(now()) {
            session = http
                .token(
                    "refresh_token",
                    &serde_json::json!({ "refresh_token": session.refresh_token }),
                )
                .map_err(|e| {
                    format!("ログインが切れました。`asis login` をやり直してください({e})")
                })?;
            save_session(&session)?;
        }
        Ok(Some(Client {
            http,
            token: session.access_token,
        }))
    }

    fn send(&self, method: &str, path: &str, body: Option<&str>) -> Result<String, String> {
        self.http.send(
            method,
            path,
            Some(&self.token),
            body,
            Some("return=representation"),
        )
    }

    pub fn rooms(&self) -> Result<Vec<Room>, String> {
        parse_rooms(&self.send("GET", ROOMS, None)?)
    }

    /// 足すか、すでにあれば書き換える
    fn upsert(&self, table: &str, conflict: &str, body: &serde_json::Value) -> Result<(), String> {
        self.http
            .send(
                "POST",
                &format!("/rest/v1/{table}?on_conflict={conflict}"),
                Some(&self.token),
                Some(&body.to_string()),
                Some("resolution=merge-duplicates,return=representation"),
            )
            .map(|_| ())
    }

    /// 変えた行が 1 つも無ければ、管理者ではない(読めない行は変えられない)
    fn changed(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        no: &str,
    ) -> Result<(), String> {
        let out = self.send(method, path, body)?;
        if out.trim() == "[]" {
            return Err(no.to_string());
        }
        Ok(())
    }

    /// 部屋を作る。作った人が管理者になる。新しい部屋の番号を返す
    pub fn create(&self, name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("部屋の名前がありません".into());
        }
        let body = serde_json::json!({ "room_name": name }).to_string();
        let out = self.send("POST", "/rest/v1/rpc/create_room", Some(&body))?;
        serde_json::from_str(&out).map_err(|_| format!("作った部屋の番号を読めません: {out}"))
    }

    /// repo を足す。`everyone` なら、メンバーでない人の分も出す(足し直せば切り替わる)
    pub fn add_repo(&self, room: &str, repo: &str, everyone: bool) -> Result<(), String> {
        let repo = repo.trim();
        let body = serde_json::json!({ "room_id": room, "repo": repo, "everyone": everyone });
        self.upsert("room_repos", "room_id,repo", &body)
            .map_err(|e| {
                if e.contains("check") {
                    format!("`{repo}` は owner/name の形ではありません")
                } else {
                    e
                }
            })
    }

    pub fn drop_repo(&self, room: &str, repo: &str) -> Result<(), String> {
        self.changed(
            "DELETE",
            &format!("/rest/v1/room_repos?room_id=eq.{room}&repo=eq.{repo}"),
            None,
            &format!("`{repo}` は外せませんでした(入っていないか、管理者ではありません)"),
        )
    }

    /// GitHub のアカウント名で招く。入っている人なら、役割を `roles` に書き換える。正しい名前を返す
    pub fn invite(&self, room: &str, login: &str, roles: &[Role]) -> Result<String, String> {
        let (github_id, login) = github_user(login.trim())?;
        let mut body =
            serde_json::json!({ "room_id": room, "github_id": github_id, "login": login });
        for role in Role::ALL {
            body[role.column()] = roles.contains(&role).into();
        }
        self.upsert("room_members", "room_id,github_id", &body)?;
        Ok(login)
    }

    /// 1 人の役割を 1 つ、付ける・外す
    pub fn set_role(&self, room: &str, github_id: i64, role: Role, on: bool) -> Result<(), String> {
        let body = serde_json::json!({ role.column(): on }).to_string();
        self.changed(
            "PATCH",
            &format!("/rest/v1/room_members?room_id=eq.{room}&github_id=eq.{github_id}"),
            Some(&body),
            "変えられませんでした(管理者ではありません)",
        )
    }

    pub fn kick(&self, room: &str, github_id: i64) -> Result<(), String> {
        self.changed(
            "DELETE",
            &format!("/rest/v1/room_members?room_id=eq.{room}&github_id=eq.{github_id}"),
            None,
            "外せませんでした(管理者ではありません)",
        )
    }

    /// 自分が出る。自分の行だけが消える
    pub fn leave(&self, room: &str) -> Result<(), String> {
        let me: i64 = self
            .send("POST", "/rest/v1/rpc/github_id", Some("{}"))?
            .trim()
            .parse()
            .map_err(|_| "自分の番号を読めません".to_string())?;
        self.changed(
            "DELETE",
            &format!("/rest/v1/room_members?room_id=eq.{room}&github_id=eq.{me}"),
            None,
            "出られませんでした(この部屋に入っていません)",
        )
    }

    /// 数えないチェックの名前を決める
    pub fn set_ignore(&self, room: &str, names: &[String]) -> Result<(), String> {
        let list: Vec<&str> = names
            .iter()
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
            .collect();
        let body = serde_json::json!({ "ignore_checks": list }).to_string();
        self.changed(
            "PATCH",
            &format!("/rest/v1/rooms?id=eq.{room}"),
            Some(&body),
            "変えられませんでした(管理者ではありません)",
        )
    }

    pub fn delete(&self, room: &str) -> Result<(), String> {
        self.changed(
            "DELETE",
            &format!("/rest/v1/rooms?id=eq.{room}"),
            None,
            "消せませんでした(管理者ではありません)",
        )
    }
}

/// 部屋の中の役割
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Review,
    Merge,
    Deploy,
    Admin,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Review, Role::Merge, Role::Deploy, Role::Admin];

    fn column(self) -> &'static str {
        match self {
            Role::Review => "review",
            Role::Merge => "merge",
            Role::Deploy => "deploy",
            Role::Admin => "is_admin",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Review => "Review",
            Role::Merge => "Merge",
            Role::Deploy => "Deploy",
            Role::Admin => "Admin",
        }
    }

    pub fn of(self, m: &Member) -> bool {
        match self {
            Role::Review => m.review,
            Role::Merge => m.merge,
            Role::Deploy => m.deploy,
            Role::Admin => m.is_admin,
        }
    }
}

fn open_browser(url: &str) {
    use std::process::{Command, Stdio};
    // `&` を含むので、コマンドの文として解釈する開き方(cmd の start)は使わない。
    let tries: [(&str, &[&str]); 5] = [
        ("wslview", &[]),
        ("xdg-open", &[]),
        ("open", &[]),
        ("rundll32.exe", &["url.dll,FileProtocolHandler"]),
        ("explorer.exe", &[]),
    ];
    for (program, args) in tries {
        if Command::new(program)
            .args(args)
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>asis</title>\
<body style=\"font-family:sans-serif;padding:3em\"><h2>asis</h2><p>";

/// ログインを待つ長さ。ブラウザで何もしなければ、ここで諦める(受け口を開けたままにしない)
const SIGN_IN_WAIT: std::time::Duration = std::time::Duration::from_secs(300);

/// GitHub でログインする。ブラウザを開き、戻りをこの PC の中で受ける。
/// `show` には、ブラウザが開かないときに人が開く URL を渡す。
pub fn sign_in(db: &Db, show: &dyn Fn(&str)) -> Result<(), String> {
    let http = Http::new(db);
    let listener = std::net::TcpListener::bind(CALLBACK)
        .map_err(|e| format!("ログインの戻りを受ける場所({CALLBACK})を開けません: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("ログインの戻りを待てません: {e}"))?;
    let deadline = std::time::Instant::now() + SIGN_IN_WAIT;
    let verifier = new_verifier()?;
    let url = format!(
        "{}/auth/v1/authorize?provider=github&redirect_to=http%3A%2F%2F{}%2Fcallback\
&code_challenge={}&code_challenge_method=s256",
        db.url,
        CALLBACK.replace(':', "%3A"),
        challenge(&verifier)
    );
    show(&url);
    open_browser(&url);
    loop {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() > deadline {
                    return Err(
                        "ログインを待ちきれませんでした。もう一度 Sign in してください".into(),
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() || !line.contains("/callback") {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n");
            continue;
        }
        let result = parse_callback(&line).and_then(|code| {
            http.token(
                "pkce",
                &serde_json::json!({ "auth_code": code, "code_verifier": verifier }),
            )
        });
        let message = match &result {
            Ok(_) => "ログインしました。この画面は閉じて大丈夫です。".to_string(),
            Err(e) => e.clone(),
        };
        let page = format!("{DONE_PAGE}{message}</p>");
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{page}",
            page.len()
        );
        return save_session(&result?);
    }
}

/// ログインの記録を消す。消したものがあれば true
pub fn sign_out() -> Result<bool, String> {
    let path = session_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// GitHub のアカウント名から、番号と正しい大文字小文字の名前を聞く。`gh` のログインを借りる。
fn github_user(login: &str) -> Result<(i64, String), String> {
    if !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || login.is_empty() {
        return Err(format!(
            "`{login}` は GitHub のアカウント名の形ではありません"
        ));
    }
    let out = std::process::Command::new("gh")
        .args([
            "api",
            &format!("users/{login}"),
            "-q",
            "[.id, .login] | @tsv",
        ])
        .output()
        .map_err(|e| format!("gh を動かせません: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    match text.trim().split_once('\t') {
        Some((id, name)) if out.status.success() => id
            .parse()
            .map(|id| (id, name.to_string()))
            .map_err(|_| format!("`{login}` の番号を読めません")),
        _ => Err(format!(
            "GitHub に `{login}` というアカウントが見つかりません"
        )),
    }
}

const HELP: &str = "asis room — 部屋(repo・メンバー・役割をまとめたもの)

  asis login                              GitHub でログインする
  asis logout                             ログインの記録を消す
  asis room                               入っている部屋を出す
  asis room new <部屋>                    部屋を作る(作った人が管理者)
  asis room add <部屋> <owner/repo>       repo を足す。メンバーの分だけ出す
                                          --everyone を付けると、メンバーでない人の分も出す(足し直せば切り替わる)
  asis room drop <部屋> <owner/repo>      repo を外す
  asis room invite <部屋> <名前> [役割]   GitHub のアカウント名で招く。入っている人なら役割を変える
                                          役割: --review --merge --deploy --admin
  asis room kick <部屋> <名前>            メンバーを外す
  asis room leave <部屋>                  自分が出る
  asis room ignore <部屋> <チェック,...>  数えないチェックの名前を決める(空にするなら \"\")
  asis room delete <部屋>                 部屋を消す

部屋に入っても、GitHub で見られない repo は見えません。";

fn yes(b: bool) -> &'static str {
    if b {
        "○"
    } else {
        "-"
    }
}

pub fn rooms_text(rooms: &[Room]) -> String {
    let mut out = String::new();
    if rooms.is_empty() {
        out.push_str("入っている部屋はありません。`asis room new <部屋>` で作れます。\n");
    }
    for room in rooms {
        out.push_str(&format!("{}\n", room.name));
        for repo in &room.repos {
            let mark = if room.everyone.contains(repo) {
                "  (全員の分を出す)"
            } else {
                ""
            };
            out.push_str(&format!("  repo      {repo}{mark}\n"));
        }
        if room.repos.is_empty() {
            out.push_str("  repo      (まだありません)\n");
        }
        out.push_str("  メンバー                 Review Merge Deploy Admin\n");
        for m in &room.members {
            let pad = 22usize.saturating_sub(m.login.chars().count());
            out.push_str(&format!(
                "    {}{}  {}      {}     {}      {}\n",
                m.login,
                " ".repeat(pad),
                yes(m.review),
                yes(m.merge),
                yes(m.deploy),
                yes(m.is_admin)
            ));
        }
        if !room.ignore_checks.is_empty() {
            out.push_str(&format!(
                "  数えないチェック  {}\n",
                room.ignore_checks.join("・")
            ));
        }
        out.push('\n');
    }
    out
}

fn need_db() -> Result<Db, String> {
    config::load()?.db.ok_or_else(|| {
        format!(
            "部屋を置く DB が設定にありません。{} に次を書いてください。\n\n[db]\nurl = \"https://….supabase.co\"\nkey = \"公開用の鍵\"",
            config::path().map(|p| p.display().to_string()).unwrap_or_default()
        )
    })
}

fn client() -> Result<Client, String> {
    Client::open(&need_db()?)?
        .ok_or_else(|| "ログインしていません。`asis login` をしてください".to_string())
}

/// `asis login` / `asis logout` / `asis room ...`
pub fn cli(args: &[String]) -> Result<(), String> {
    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with("--"))
        .collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    if let Some(bad) = args.iter().find(|a| {
        a.starts_with("--")
            && !["--review", "--merge", "--deploy", "--admin", "--everyone"].contains(&a.as_str())
    }) {
        return Err(format!("知らない指定です: {bad}\n\n{HELP}"));
    }
    match words.as_slice() {
        ["login"] => {
            sign_in(&need_db()?, &|url| {
                println!("ブラウザで GitHub のログインを開きます。開かないときは、次を開いてください。\n\n{url}\n")
            })?;
            println!("ログインしました。");
            Ok(())
        }
        ["logout"] => {
            println!(
                "{}",
                if sign_out()? {
                    "ログインの記録を消しました。"
                } else {
                    "ログインしていません。"
                }
            );
            Ok(())
        }
        ["room"] => {
            print!("{}", rooms_text(&client()?.rooms()?));
            Ok(())
        }
        ["room", "new", name] => {
            let c = client()?;
            c.create(name)?;
            print!("{}", rooms_text(&c.rooms()?));
            Ok(())
        }
        ["room", action, name, rest @ ..] => {
            let c = client()?;
            let rooms = c.rooms()?;
            let room = find(&rooms, name)?;
            let id = &room.id;
            match (*action, rest) {
                ("add", [repo]) => c.add_repo(id, repo, flag("--everyone"))?,
                ("drop", [repo]) => c.drop_repo(id, repo)?,
                ("invite", [login]) => {
                    let roles: Vec<Role> = Role::ALL
                        .into_iter()
                        .filter(|r| flag(&format!("--{}", r.label().to_lowercase())))
                        .collect();
                    c.invite(id, login, &roles)?;
                }
                ("kick", [login]) => {
                    let member = room
                        .members
                        .iter()
                        .find(|m| m.login.eq_ignore_ascii_case(login))
                        .ok_or(format!("`{login}` はこの部屋にいません"))?;
                    c.kick(id, member.github_id)?;
                }
                ("leave", []) => c.leave(id)?,
                ("ignore", [names]) => {
                    let list: Vec<String> = names.split(',').map(str::to_string).collect();
                    c.set_ignore(id, &list)?;
                }
                ("delete", []) => c.delete(id)?,
                _ => return Err(HELP.to_string()),
            }
            print!("{}", rooms_text(&c.rooms()?));
            Ok(())
        }
        _ => Err(HELP.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[{
      "id": "11111111-1111-1111-1111-111111111111", "name": "team",
      "merge_by_author": false, "deploy_by_author": true, "ignore_checks": ["Preview"],
      "room_repos": [{"repo": "o/web", "everyone": true}, {"repo": "o/api", "everyone": false}],
      "room_members": [
        {"github_id": 2, "login": "bob", "is_admin": false, "review": false, "merge": false, "deploy": false},
        {"github_id": 1, "login": "Alice", "is_admin": true, "review": true, "merge": true, "deploy": false}
      ]
    }]"#;

    #[test]
    fn room_becomes_a_project_and_a_list_of_people() {
        let rooms = parse_rooms(SAMPLE).unwrap();
        let room = find(&rooms, "team").unwrap();
        assert_eq!(room.repos, vec!["o/api", "o/web"]);
        // 既定はメンバーの分だけ。「全員」にした repo は絞らない
        assert_eq!(room.people("o/api"), vec!["Alice", "bob"]);
        assert!(room.people("o/web").is_empty());
        let p = room.project();
        assert_eq!((p.name.as_str(), p.repos.len()), ("team", 2));
        assert_eq!(p.members, vec!["Alice", "bob"]);
        assert_eq!(p.review, vec!["Alice"]);
        assert_eq!(p.merge, vec!["Alice"]);
        // 本番に出す役の人がいなくても、「出した本人」にしていればそれが入る
        assert_eq!(p.deploy, vec![AUTHOR]);
        assert_eq!(p.ignore_checks, vec!["Preview"]);
        assert_eq!(
            p.room.as_deref(),
            Some("11111111-1111-1111-1111-111111111111")
        );
    }

    #[test]
    fn find_needs_exactly_one_room_of_that_name() {
        let mut rooms = parse_rooms(SAMPLE).unwrap();
        assert!(find(&rooms, "nope").unwrap_err().contains("ありません"));
        rooms.push(rooms[0].clone());
        assert!(find(&rooms, "team").unwrap_err().contains("2 つ以上"));
        assert!(parse_rooms("{\"message\":\"x\"}").is_err());
        assert_eq!(parse_rooms("[]").unwrap(), vec![]);
    }

    #[test]
    fn challenge_is_the_url_safe_hash_of_the_verifier() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mBKWvS4T_Q0ccEWp4hTR-WkSGWkQGY"),
            "MqgnGXrg0jmH_YCycI0HpSbV0jylH2UiqxN-7SdELko"
        );
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        let (a, b) = (new_verifier().unwrap(), new_verifier().unwrap());
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
    }

    #[test]
    fn callback_gives_the_code_or_the_reason() {
        assert_eq!(
            parse_callback("GET /callback?code=abc-123&x=1 HTTP/1.1").unwrap(),
            "abc-123"
        );
        assert!(parse_callback(
            "GET /callback?error=access_denied&error_description=The+user+%E6%8B%92 HTTP/1.1"
        )
        .unwrap_err()
        .contains("The user 拒"));
        assert!(parse_callback("GET /callback HTTP/1.1").is_err());
        assert!(parse_callback("GET /callback?code= HTTP/1.1").is_err());
        assert!(parse_callback("GET /favicon.ico HTTP/1.1").is_err());
    }

    #[test]
    fn session_is_stale_a_minute_before_it_expires() {
        let s = Session {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1000,
        };
        assert!(!s.is_stale(900));
        assert!(s.is_stale(950));
        assert!(s.is_stale(2000));
    }

    #[test]
    fn failures_are_explained_in_one_line() {
        assert!(explain(
            403,
            r#"{"code":"42501","message":"new row violates row-level security policy"}"#
        )
        .contains("管理者だけ"));
        assert!(explain(
            403,
            r#"{"code":"42501","message":"この DB では、部屋を作る許可がありません"}"#
        )
        .contains("許可がありません"));
        assert_eq!(
            explain(409, r#"{"code":"23505","message":"duplicate key"}"#),
            "もう入っています"
        );
        assert!(explain(400, r#"{"error_description":"bad code"}"#).contains("bad code"));
        assert!(explain(502, "upstream down").contains("upstream down (HTTP 502)"));
    }

    #[test]
    fn rooms_are_listed_with_roles() {
        let text = rooms_text(&parse_rooms(SAMPLE).unwrap());
        assert!(text.contains("repo      o/api\n"), "{text}");
        assert!(text.contains("repo      o/web  (全員の分を出す)"), "{text}");
        assert!(
            text.lines()
                .any(|l| l.contains("Alice") && l.matches('○').count() == 3),
            "{text}"
        );
        assert!(
            text.lines().any(|l| l.contains("bob") && !l.contains('○')),
            "{text}"
        );
        assert!(rooms_text(&[]).contains("asis room new"));
    }
}
