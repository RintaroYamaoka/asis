//! 前に大きく出る中身(PR 1 本・issue)と、書き込む欄

use super::*;
use iced::widget::{column, row};

pub fn notice_of<'a>(app: &App) -> Option<Element<'a, Msg>> {
    app.notice.as_ref().map(|(t, ok)| {
        text(t.clone())
            .size(13)
            .color(if *ok { app.accent } else { BAD })
            .into()
    })
}

/// 選んだ 1 件の中身。前に大きく出す。
/// 段の頭を押したら issue 1 件、カードを押したら PR 1 本。操作は PR 1 本の中でする。
pub fn detail_of(app: &App) -> Option<Element<'_, Msg>> {
    let (v, r) = current(app)?;
    let key = key_of(v, r);
    let me = |turn: &[String]| -> (String, bool) {
        if turn.contains(&v.viewer) {
            ("you".to_string(), true)
        } else if turn.is_empty() {
            ("no one".to_string(), false)
        } else {
            (turn.join(", "), false)
        }
    };
    let why = |state: String, next: String, age: &str| -> Element<'_, Msg> {
        container(
            text(if age.is_empty() || next == "no one" {
                format!("{state} · Waiting on {next}")
            } else {
                format!("{state} · Waiting on {next} for {age}")
            })
            .size(14)
            .color(TEXT)
            .wrapping(text::Wrapping::WordOrGlyph),
        )
        .padding([12, 14])
        .width(Length::Fill)
        .style(boxed(FRAME, Color::TRANSPARENT))
        .into()
    };
    let at = app
        .pr
        .and_then(|n| r.prs.iter().position(|p| p.number == n));
    let mut d = column![].spacing(14);
    match at {
        // ---- PR 1 本 ----
        Some(at) => {
            let p = &r.prs[at];
            let (next, _) = me(&p.turn);
            let mut top = row![cap(format!("{} · PR #{}", v.repo, p.number))]
                .spacing(10)
                .align_y(Alignment::Center);
            if matches!(r.kind, Kind::Task | Kind::Doc) {
                top = top.push(seg(
                    if r.kind == Kind::Task {
                        format!("issue #{}", r.number)
                    } else {
                        r.label.clone()
                    },
                    false,
                    Msg::Select(key.clone(), None),
                ));
            } else {
                top = top.push(cap("No linked issue"));
            }
            top = top.push(Space::new().width(Length::Fill));
            let n = r.prs.len();
            if n > 1 {
                let go = |by: usize| Msg::Select(key.clone(), Some(r.prs[(at + by) % n].number));
                top = top
                    .push(seg("‹".into(), false, go(n - 1)))
                    .push(cap(format!("{} / {n}", at + 1)))
                    .push(seg("›".into(), false, go(1)));
            }
            d = d
                .push(top)
                .push(
                    text(p.title.clone())
                        .size(20)
                        .color(TEXT)
                        .wrapping(text::Wrapping::WordOrGlyph),
                )
                .push(why(p.why.clone(), next, &p.age));
            // GitHub の PR のページに出ているもの:誰が・どのブランチから・どれくらいの大きさで
            let (added, removed, files) = p.size;
            let mut facts = vec![format!("by {}", p.author), p.branch.clone()];
            if files > 0 {
                facts.push(format!("+{added} −{removed} · {files} files"));
            }
            d = d.push(cap(facts.join("  ·  ")));
            if !p.reviewers.is_empty() {
                d = d.push(cap(format!("Review requested: {}", p.reviewers.join(", "))));
            }
            if !p.failing.is_empty() {
                d = d.push(cap(format!("Checks failing: {}", p.failing.join(", "))).color(BAD));
            }
            if let Some(n) = notice_of(app) {
                d = d.push(n);
            }
            // その PR の今の事実で、できる操作だけを出す
            let repo = v.repo.clone();
            let number = p.number;
            let free = !app.busy;
            let merge = |strong: bool| -> Element<'_, Msg> {
                if app.confirm == Some(number) {
                    primary(
                        "Confirm merge",
                        app.accent,
                        free.then_some(Msg::Act(asis::act::Action::Merge {
                            repo: repo.clone(),
                            number,
                        })),
                    )
                } else if strong {
                    primary("Merge", app.accent, Some(Msg::AskMerge(number)))
                } else {
                    seg("Merge".into(), false, Msg::AskMerge(number))
                }
            };
            if p.mark == "?" {
                if app.fixing == Some(number) {
                    let send = Msg::Act(asis::act::Action::RequestChanges {
                        repo: repo.clone(),
                        number,
                        comment: app.comment.clone(),
                    });
                    d = d.push(
                        row![
                            field(
                                "What needs to change?",
                                &app.comment,
                                Msg::Comment,
                                send.clone()
                            ),
                            primary("Request changes", app.accent, free.then_some(send)),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    );
                } else {
                    d = d.push(
                        row![
                            primary(
                                "Approve",
                                app.accent,
                                free.then_some(Msg::Act(asis::act::Action::Approve {
                                    repo: repo.clone(),
                                    number
                                }))
                            ),
                            seg("Request changes…".into(), false, Msg::AskFix(number)),
                            merge(false),
                        ]
                        .spacing(8),
                    );
                }
            } else if p.mark == "+" {
                d = d.push(merge(true));
            }
            for l in &r.local {
                d = d.push(cap(format!("Local  {}  {}", l.branch, l.panes.join(" / "))));
            }
            // 本文は、書かれたまま
            d = d.push(cap("Description")).push(
                text(if p.body.trim().is_empty() {
                    "No description".to_string()
                } else {
                    p.body.clone()
                })
                .size(14)
                .color(DIM)
                .wrapping(text::Wrapping::WordOrGlyph),
            );
            d = d.push(seg(
                format!("Open PR #{number} on GitHub"),
                false,
                Msg::Open(format!("https://github.com/{}/pull/{number}", v.repo)),
            ));
        }
        // ---- issue 1 件(手元の作業の行も、ここ)----
        None => {
            let (next, _) = me(&r.turn);
            d = d
                .push(cap(format!("{} · {}", v.repo, label_of(r))))
                .push(
                    text(r.title.clone())
                        .size(20)
                        .color(TEXT)
                        .wrapping(text::Wrapping::WordOrGlyph),
                )
                .push(why(r.why.clone(), next, &r.age));
            if let Some(n) = notice_of(app) {
                d = d.push(n);
            }
            if matches!(r.kind, Kind::Task | Kind::Doc) {
                d = d.push(cap("Description")).push(
                    text(if r.body.trim().is_empty() {
                        "No description".to_string()
                    } else {
                        r.body.clone()
                    })
                    .size(14)
                    .color(DIM)
                    .wrapping(text::Wrapping::WordOrGlyph),
                );
            }
            if r.prs.is_empty() {
                d = d.push(cap("No pull requests yet"));
            } else {
                d = d.push(cap(format!("{} pull requests", r.prs.len())));
                for p in &r.prs {
                    let (who, mine) = me(&p.turn);
                    let one = column![
                        row![
                            cap(format!("#{}", p.number)),
                            chip(p),
                            Space::new().width(Length::Fill),
                            text(who)
                                .size(13)
                                .color(if mine { app.accent } else { DIM }),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                        text(p.title.clone())
                            .size(15)
                            .color(TEXT)
                            .wrapping(text::Wrapping::WordOrGlyph),
                    ]
                    .spacing(6);
                    d = d.push(pressable(
                        one.into(),
                        FRAME,
                        false,
                        [11, 14],
                        Msg::Select(key.clone(), Some(p.number)),
                    ));
                }
            }
            for l in &r.local {
                d = d.push(cap(format!("Local  {}  {}", l.branch, l.panes.join(" / "))));
            }
            if !r.url.is_empty() {
                d = d.push(seg(
                    format!(
                        "Open {} on GitHub",
                        if r.kind == Kind::Task {
                            format!("issue #{}", r.number)
                        } else {
                            label_of(r)
                        }
                    ),
                    false,
                    Msg::Open(r.url.clone()),
                ));
            }
        }
    }
    Some(
        scrollable(container(d).padding(24).width(Length::Fill))
            .width(760)
            .into(),
    )
}

/// issue を書く。1 行でよい。型は無い
pub fn write_form(app: &App) -> Option<Element<'_, Msg>> {
    let w = app.write.as_ref()?;
    let send = Msg::Act(asis::act::Action::WriteIssue {
        repo: w.repo.clone().unwrap_or_default(),
        title: w.title.clone(),
        body: w.body.clone(),
    });
    let ready = !app.busy && w.repo.is_some() && !w.title.trim().is_empty();
    // 出し先は選ばせない。いま見ている repository に出す
    let mut d = column![
        row![
            text("New issue").size(18).color(TEXT),
            cap(format!("in {}", w.repo.as_deref().unwrap_or_default())),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        cap(app.t(
            "1 行でよい。型は無い。書いたものが、そのまま AI への指示になる。",
            "One line is enough. No template. What you write becomes the instruction for the AI."
        )),
        field("Title", &w.title, Msg::WriteTitle, send.clone()),
        field(
            "Description (optional)",
            &w.body,
            Msg::WriteBody,
            send.clone()
        ),
    ]
    .spacing(12);
    if let Some(n) = notice_of(app) {
        d = d.push(n);
    }
    d = d.push(
        row![
            Space::new().width(Length::Fill),
            seg("Cancel".into(), false, Msg::Close),
            primary("Create", app.accent, ready.then_some(send)),
        ]
        .spacing(8),
    );
    Some(container(d).padding(24).width(600).into())
}
