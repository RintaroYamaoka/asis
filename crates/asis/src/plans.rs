//! 外から送られてくる計画。
//! 計画の正本が GitHub に無い人(手元の文書に書いている人)が、その人の道具から、部屋に計画の一覧を送る。
//! 送る側に形を求めない。来た 1 件を、そのまま持つ。こちらが読むのは「どれが番号か」だけで、
//! 題・状態などの欄は、出すときに、ありそうな名前から探す。
use crate::room::{find, Client, Room};
use crate::{lang, say};
use serde_json::Value;

/// 送られてきた計画 1 件
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// 番号(`T-12` など)。部屋の中で 1 つ
    pub key: String,
    /// 送られてきたまま
    pub data: Value,
}

/// 番号が入っていそうな欄の名前
const KEY: &[&str] = &["番号", "number", "key", "id", "no"];
pub const TITLE: &[&str] = &["題", "題名", "タイトル", "title", "name"];
pub const STATE: &[&str] = &["状態", "state", "status"];
pub const STEP: &[&str] = &["段", "step", "stage", "phase"];
pub const WAITS: &[&str] = &["待つもの", "待ち", "waits", "waiting", "blocked_by"];

/// 1 件の中から、名前のどれかに当たる欄を、字にして返す。並びは `、` でつなぐ。空は無いのと同じ
pub fn field(data: &Value, names: &[&str]) -> Option<String> {
    let text = |v: &Value| match v {
        Value::String(s) => s.trim().to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    let found = names.iter().find_map(|name| data.get(name))?;
    let out = match found {
        Value::Array(items) => items
            .iter()
            .map(text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("、"),
        other => text(other),
    };
    (!out.is_empty()).then_some(out)
}

/// 送られてきた字を、計画の並びに読む。
/// 並びそのもの・`{"plans": [...]}` のように 1 つの欄に並びが入ったもの・1 件だけ、のどれでも受ける。
/// 番号の無いもの・同じ番号が 2 つあるものは、どれかを黙って捨てずに、理由を返す
pub fn parse(text: &str) -> Result<Vec<Plan>, String> {
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| say!("JSON として読めません: {e}", "Cannot read it as JSON: {e}"))?;
    let items: Vec<Value> = match value {
        Value::Array(items) => items,
        Value::Object(map) => {
            let mut lists = map.values().filter(|v| v.is_array());
            match (lists.next(), lists.next()) {
                (Some(Value::Array(items)), None)
                    if field(&Value::Object(map.clone()), KEY).is_none() =>
                {
                    items.clone()
                }
                _ => vec![Value::Object(map)],
            }
        }
        _ => {
            return Err(say!(
                "計画の並びではありません",
                "This is not a list of plans"
            ))
        }
    };
    let mut plans: Vec<Plan> = Vec::new();
    for (i, data) in items.into_iter().enumerate() {
        let at = i + 1;
        let Some(key) = field(&data, KEY).filter(|_| data.is_object()) else {
            return Err(say!(
                "{at} 件目に番号がありません(番号・number・key・id のどれかの欄に入れてください)",
                "Item {at} has no number (put it in one of: 番号, number, key, id)"
            ));
        };
        if key.chars().count() > 80 {
            return Err(say!(
                "{at} 件目の番号が長すぎます(80 字まで)",
                "The number of item {at} is too long (80 characters at most)"
            ));
        }
        if plans.iter().any(|p| p.key == key) {
            return Err(say!(
                "番号 {key} が 2 つあります",
                "The number {key} appears twice"
            ));
        }
        plans.push(Plan { key, data });
    }
    Ok(plans)
}

/// DB の返事(行の並び)を読む
pub fn parse_rows(text: &str) -> Result<Vec<Plan>, String> {
    #[derive(serde::Deserialize)]
    struct Row {
        key: String,
        data: Value,
    }
    let rows: Vec<Row> = serde_json::from_str(text).map_err(|e| {
        say!(
            "計画の一覧を読めません: {e}",
            "Cannot read the list of plans: {e}"
        )
    })?;
    Ok(rows
        .into_iter()
        .map(|r| Plan {
            key: r.key,
            data: r.data,
        })
        .collect())
}

/// 1 件を 1 行に(番号・題・状態・段・待つもの。無い欄は出さない)
pub fn line(plan: &Plan) -> String {
    let mut out = plan.key.clone();
    for (names, label) in [(TITLE, ""), (STATE, ""), (STEP, ""), (WAITS, "← ")] {
        if let Some(text) = field(&plan.data, names) {
            out.push_str("  ");
            out.push_str(label);
            out.push_str(&text);
        }
    }
    out
}

const HELP: &str = "asis plans — 部屋に送られた計画

  asis plans [部屋]                 部屋の計画を出す
  asis plans push <ファイル> [部屋]   計画を送る(JSON。ファイルを - にすると、流し込まれたものを読む)

送る JSON は、計画の並び。1 件に番号の欄(番号・number・key・id のどれか)があれば、ほかの欄は自由。
同じ番号は書き換える。送らなかった番号は、そのまま残る。
部屋は、入っている部屋が 1 つなら書かなくてよい。";

/// 部屋を選ぶ。名前が無ければ、入っている部屋が 1 つのときだけ決まる
fn pick<'a>(rooms: &'a [Room], name: Option<&str>) -> Result<&'a Room, String> {
    match (name, rooms) {
        (Some(name), _) => find(rooms, name),
        (None, [one]) => Ok(one),
        (None, []) => {
            Err(lang::t("入っている部屋がありません", "You are not in any Room").to_string())
        }
        (None, _) => {
            let names = rooms
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Err(say!(
                "部屋が 2 つ以上あります。名前を書いてください: {names}",
                "You are in more than one Room. Name one: {names}"
            ))
        }
    }
}

/// `asis plans ...`
pub fn cli(args: &[String], client: &Client) -> Result<(), String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["plans", "push", file, rest @ ..] if rest.len() <= 1 => {
            let text = if *file == "-" {
                std::io::read_to_string(std::io::stdin()).map_err(|e| e.to_string())?
            } else {
                std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?
            };
            let plans = parse(&text)?;
            let rooms = client.rooms()?;
            let room = pick(&rooms, rest.first().copied())?;
            client.push_plans(&room.id, &plans)?;
            let (name, n) = (&room.name, plans.len());
            println!(
                "{}",
                say!(
                    "{name} に計画を {n} 件 送りました",
                    "Sent {n} plans to {name}"
                )
            );
            Ok(())
        }
        ["plans", rest @ ..] if rest.len() <= 1 && rest.first() != Some(&"help") => {
            let rooms = client.rooms()?;
            let room = pick(&rooms, rest.first().copied())?;
            let plans = client.plans(&room.id)?;
            if plans.is_empty() {
                println!("{}", lang::t("計画はまだ送られていません", "No plans yet"));
            }
            for plan in &plans {
                println!("{}", line(plan));
            }
            Ok(())
        }
        _ => Err(HELP.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_list_of_plans_is_read_whatever_the_fields_are_called() {
        let plans = parse(
            r#"[{"番号":"T-1","題":"検索を速くする","状態":"進行中","段":"2","待つもの":["T-3","T-4"]},
                {"number":"T-2","title":"Faster search","status":"done"}]"#,
        )
        .unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].key, "T-1");
        assert_eq!(
            field(&plans[0].data, TITLE).as_deref(),
            Some("検索を速くする")
        );
        assert_eq!(field(&plans[0].data, STATE).as_deref(), Some("進行中"));
        assert_eq!(field(&plans[0].data, WAITS).as_deref(), Some("T-3、T-4"));
        assert_eq!(plans[1].key, "T-2");
        assert_eq!(field(&plans[1].data, STATE).as_deref(), Some("done"));
        // 無い欄・空の欄は、無いものとして扱う
        assert_eq!(field(&plans[1].data, WAITS), None);
        assert_eq!(field(&json!({"待つもの": []}), WAITS), None);
        assert_eq!(field(&json!({"状態": "  "}), STATE), None);
        // 送られてきたものは、欄を落とさずそのまま持つ
        assert_eq!(plans[0].data["段"], "2");
    }

    #[test]
    fn a_wrapped_list_a_single_plan_and_a_numeric_number_are_all_accepted() {
        let wrapped = parse(r#"{"plans":[{"key":"A-1"},{"key":"A-2"}]}"#).unwrap();
        assert_eq!(wrapped.len(), 2);
        // 1 件だけ。中に並びの欄(待つもの)があっても、それを計画の並びと取り違えない
        let one = parse(r#"{"番号":"A-1","待つもの":["A-2"]}"#).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].key, "A-1");
        assert_eq!(parse(r#"[{"id":12}]"#).unwrap()[0].key, "12");
        assert_eq!(parse("\u{feff}[]").unwrap(), vec![]);
    }

    #[test]
    fn nothing_is_dropped_silently() {
        assert!(parse(r#"[{"番号":"A-1"},{"題":"番号が無い"}]"#)
            .unwrap_err()
            .contains('2'));
        assert!(parse(r#"[{"番号":"A-1"},{"番号":"A-1"}]"#)
            .unwrap_err()
            .contains("A-1"));
        assert!(parse(r#"["A-1"]"#).is_err());
        assert!(parse("12").is_err());
        assert!(parse("{").is_err());
        assert!(parse(&format!(r#"[{{"番号":"{}"}}]"#, "x".repeat(81))).is_err());
    }

    #[test]
    fn a_plan_is_shown_on_one_line_with_only_the_fields_it_has() {
        let plan = |v: Value| Plan {
            key: "T-1".into(),
            data: v,
        };
        assert_eq!(
            line(&plan(
                json!({"題":"検索を速くする","状態":"進行中","段":"2","待つもの":["T-3"]})
            )),
            "T-1  検索を速くする  進行中  2  ← T-3"
        );
        assert_eq!(line(&plan(json!({}))), "T-1");
    }

    #[test]
    fn rows_from_the_database_are_read_back() {
        let plans = parse_rows(r#"[{"key":"T-1","data":{"題":"a"}}]"#).unwrap();
        assert_eq!(plans[0].key, "T-1");
        assert_eq!(field(&plans[0].data, TITLE).as_deref(), Some("a"));
        assert!(parse_rows("{").is_err());
    }
}
