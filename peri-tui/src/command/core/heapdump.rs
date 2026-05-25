use std::io::Write;

use crate::app::App;
use crate::command::Command;

pub struct HeapdumpCommand;

impl Command for HeapdumpCommand {
    fn name(&self) -> &str {
        "heapdump"
    }

    fn description(&self, _lc: &crate::i18n::LcRegistry) -> String {
        "Dump heap memory profile to .tmp/heapdump-*.txt".to_string()
    }

    fn execute(&self, app: &mut App, _args: &str) {
        let now = chrono::Local::now();
        let filename = format!(".tmp/heapdump-{}.txt", now.format("%Y%m%d-%H%M%S"));

        let mut buf: Vec<u8> = Vec::new();

        // ── 1. RSS ──
        let rss_mb = read_rss_mb();
        let _ = writeln!(buf, "=== HEAPDUMP {} ===", now.format("%Y-%m-%d %H:%M:%S"));
        let _ = writeln!(buf, "RSS: {:.1} MB\n", rss_mb);

        // ── 2. Allocator info ──
        #[cfg(not(target_os = "windows"))]
        {
            let _ = writeln!(buf, "=== ALLOCATOR ===");
            let _ = writeln!(buf, "  backend: mimalloc");
            let _ = writeln!(
                buf,
                "  note: mimalloc automatically returns freed pages to the OS"
            );
            let _ = writeln!(buf);
        }

        // ── 3. TUI components ──
        {
            let s = &app.session_mgr.sessions[app.session_mgr.active];
            let agent_bytes: usize = s
                .agent
                .agent_state_messages
                .iter()
                .map(|m| m.content().len())
                .sum();
            let pipeline_bytes: usize = s
                .messages
                .pipeline
                .completed_messages()
                .iter()
                .map(|m| m.content().len())
                .sum();

            let _ = writeln!(buf, "\n=== TUI COMPONENTS ===");
            let _ = writeln!(
                buf,
                "  agent_state_messages: count={}, bytes={:.1}MB",
                s.agent.agent_state_messages.len(),
                agent_bytes as f64 / (1024.0 * 1024.0)
            );
            let _ = writeln!(
                buf,
                "  pipeline_completed:   count={}, bytes={:.1}MB",
                s.messages.pipeline.completed_messages().len(),
                pipeline_bytes as f64 / (1024.0 * 1024.0)
            );
            let _ = writeln!(
                buf,
                "  view_messages:        count={}",
                s.messages.view_messages.len()
            );
            let _ = writeln!(
                buf,
                "  pending_messages:     count={}",
                s.messages.pending_messages.len()
            );
            let _ = writeln!(
                buf,
                "  ephemeral_notes:      count={}",
                s.messages.ephemeral_notes.len()
            );
            let _ = writeln!(buf, "  todo_items:           count={}", s.todo_items.len());
            let _ = writeln!(
                buf,
                "  background_tasks:     count={}",
                app.session_mgr.sessions[app.session_mgr.active].background_task_count
            );
        }

        // ── 4. All sessions ──
        {
            let _ = writeln!(buf, "\n=== SESSIONS ===");
            for (i, sess) in app.session_mgr.sessions.iter().enumerate() {
                let _ = writeln!(
                    buf,
                    "  [{}]: agent_msgs={}, view_vms={}, loading={}",
                    i,
                    sess.agent.agent_state_messages.len(),
                    sess.messages.view_messages.len(),
                    sess.ui.loading,
                );
            }
        }

        // Write file
        let full_path = std::path::Path::new(&filename);
        if let Some(parent) = full_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let msg = match std::fs::write(full_path, &buf) {
            Ok(()) => format!("Heapdump -> {filename}\nRSS: {rss_mb:.0}MB"),
            Err(e) => format!("heapdump failed: {e}"),
        };
        app.session_mgr.sessions[app.session_mgr.active]
            .messages
            .view_messages
            .push(crate::app::MessageViewModel::system(msg));
    }
}

fn read_rss_mb() -> f64 {
    if cfg!(target_os = "macos") {
        std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .map(|kb| kb / 1024.0)
            })
            .unwrap_or(-1.0)
    } else {
        -1.0
    }
}
