use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}

#[derive(Debug, Clone)]
pub struct SubRow {
    pub icon: String,
    pub glyph: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct Todo {
    pub status: TodoStatus,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Activity {
    pub subagents: Vec<SubRow>,
    pub todos: Vec<Todo>,
}

#[derive(Deserialize)]
struct RawSub {
    icon: Option<String>,
    glyph: Option<String>,
    title: Option<String>,
}

#[derive(Deserialize)]
struct RawTodo {
    status: Option<String>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct RawState {
    zsession: Option<String>,
    name: Option<String>,
    subagents: Option<std::collections::BTreeMap<String, RawSub>>,
    todos: Option<Vec<RawTodo>>,
}

fn status_from(s: &str) -> TodoStatus {
    match s {
        "pending" => TodoStatus::Pending,
        "in_progress" => TodoStatus::InProgress,
        "done" => TodoStatus::Done,
        _ => TodoStatus::Pending,
    }
}

fn checkbox(s: TodoStatus) -> &'static str {
    match s {
        TodoStatus::Done => "☑",
        TodoStatus::InProgress => "▣",
        _ => "☐",
    }
}

pub fn parse_activity(payload: &str) -> Option<(String, String, Activity)> {
    let raw: RawState = serde_json::from_str(payload).ok()?;
    let name = raw.name?;
    let zsession = raw.zsession.unwrap_or_default();
    let subagents = raw
        .subagents
        .unwrap_or_default()
        .into_values()
        .map(|s| SubRow {
            icon: s.icon.unwrap_or_else(|| "⊜".into()),
            glyph: s.glyph.unwrap_or_else(|| "⚙".into()),
            title: s.title.unwrap_or_default(),
        })
        .collect();
    let todos = raw
        .todos
        .unwrap_or_default()
        .into_iter()
        .map(|t| Todo {
            status: status_from(t.status.as_deref().unwrap_or("")),
            text: t.text.unwrap_or_default(),
        })
        .collect();
    Some((zsession, name, Activity { subagents, todos }))
}

fn truncate(s: &str, cols: usize) -> String {
    if s.chars().count() <= cols {
        return s.to_string();
    }
    s.chars().take(cols).collect()
}

pub fn render_activity(a: &Activity, cols: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if !a.subagents.is_empty() {
        for s in &a.subagents {
            let row = if s.title.is_empty() {
                format!("  {} {}", s.icon, s.glyph)
            } else {
                format!("  {} {} {}", s.icon, s.glyph, s.title)
            };
            out.push(truncate(&row, cols));
        }
        return out;
    }
    let visible: Vec<&Todo> = a
        .todos
        .iter()
        .filter(|t| t.status != TodoStatus::Done)
        .collect();
    let cap = 6usize;
    for t in visible.iter().take(cap) {
        out.push(truncate(
            &format!("  {} {}", checkbox(t.status), t.text),
            cols,
        ));
    }
    if visible.len() > cap {
        out.push(truncate("  …", cols));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_subagents_with_title() {
        let payload = r#"{"zsession":"sessA","name":"login-fix","subagents":{"a1":{"icon":"⌕","glyph":"◉","title":"explore auth","last_seen":1}},"todos":[]}"#;
        let (zsession, name, act) = parse_activity(payload).unwrap();
        assert_eq!(zsession, "sessA");
        assert_eq!(name, "login-fix");
        assert_eq!(act.subagents.len(), 1);
        assert_eq!(act.subagents[0].icon, "⌕");
        assert_eq!(act.subagents[0].glyph, "◉");
        assert_eq!(act.subagents[0].title, "explore auth");
    }

    #[test]
    fn renders_subagent_row_with_title() {
        let act = Activity {
            subagents: vec![SubRow {
                icon: "⌕".into(),
                glyph: "◉".into(),
                title: "explore auth".into(),
            }],
            todos: vec![],
        };
        assert_eq!(
            render_activity(&act, 30),
            vec!["  ⌕ ◉ explore auth".to_string()]
        );
    }

    #[test]
    fn renders_subagent_row_without_title() {
        let act = Activity {
            subagents: vec![SubRow {
                icon: "◆".into(),
                glyph: "⚡".into(),
                title: "".into(),
            }],
            todos: vec![],
        };
        assert_eq!(render_activity(&act, 30), vec!["  ◆ ⚡".to_string()]);
    }

    #[test]
    fn renders_todos_with_checkboxes_dropping_done() {
        let act = Activity {
            subagents: vec![],
            todos: vec![
                Todo {
                    status: TodoStatus::Done,
                    text: "scaffold".into(),
                },
                Todo {
                    status: TodoStatus::InProgress,
                    text: "run tests".into(),
                },
                Todo {
                    status: TodoStatus::Pending,
                    text: "migrate".into(),
                },
            ],
        };
        let rows = render_activity(&act, 40);
        assert_eq!(
            rows,
            vec!["  ▣ run tests".to_string(), "  ☐ migrate".to_string(),]
        );
    }

    #[test]
    fn all_done_renders_nothing() {
        let act = Activity {
            subagents: vec![],
            todos: vec![Todo {
                status: TodoStatus::Done,
                text: "x".into(),
            }],
        };
        assert!(render_activity(&act, 40).is_empty());
    }

    #[test]
    fn caps_todos_with_overflow() {
        let todos: Vec<Todo> = (0..9)
            .map(|i| Todo {
                status: TodoStatus::Pending,
                text: format!("t{}", i),
            })
            .collect();
        let act = Activity {
            subagents: vec![],
            todos,
        };
        let rows = render_activity(&act, 40);
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[6], "  …");
    }

    #[test]
    fn subagents_take_priority_over_todos() {
        let act = Activity {
            subagents: vec![SubRow {
                icon: "◆".into(),
                glyph: "✎".into(),
                title: "fix".into(),
            }],
            todos: vec![Todo {
                status: TodoStatus::Pending,
                text: "x".into(),
            }],
        };
        assert_eq!(render_activity(&act, 24), vec!["  ◆ ✎ fix".to_string()]);
    }

    #[test]
    fn truncates_to_cols() {
        let act = Activity {
            subagents: vec![],
            todos: vec![Todo {
                status: TodoStatus::Pending,
                text: "a-very-long-task-name-here".into(),
            }],
        };
        for r in &render_activity(&act, 10) {
            assert!(r.chars().count() <= 10, "row too wide: {:?}", r);
        }
    }

    #[test]
    fn empty_activity_renders_nothing() {
        assert!(render_activity(&Activity::default(), 30).is_empty());
    }

    #[test]
    fn unknown_status_defaults_to_pending() {
        let act = Activity {
            subagents: vec![],
            todos: vec![Todo {
                status: status_from("bogus"),
                text: "x".into(),
            }],
        };
        assert_eq!(render_activity(&act, 20), vec!["  ☐ x".to_string()]);
    }
}
