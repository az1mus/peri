//! IME composition window positioning.
//!
//! On terminal emulators, the IME composition window position is determined by
//! the terminal cursor position. If the terminal cursor stays at (0, 0) — the
//! top-left corner — the IME candidate window appears there instead of following
//! the text input box.
//!
//! This module calculates the textarea cursor's terminal-coordinate position.
//! The render loop calls `Frame::set_cursor` with this position so the terminal
//! knows where to anchor the IME composition window.

use ratatui::layout::Rect;
use tui_textarea::TextArea;

/// Calculate the terminal-grid position of the visible textarea cursor.
///
/// Returns `None` if the textarea has zero visible area.
pub fn textarea_cursor_pos(textarea: &TextArea, textarea_area: Rect) -> Option<(u16, u16)> {
    let visible_height = textarea_area.height as usize;
    let visible_width = textarea_area.width as usize;
    if visible_height == 0 || visible_width == 0 {
        return None;
    }

    let (cursor_row, cursor_col) = textarea.cursor();

    // Vertical scroll: cursor is always kept within the visible area
    let scroll_row = cursor_row.saturating_sub(visible_height.saturating_sub(1));
    let visible_row = cursor_row.saturating_sub(scroll_row);

    // Horizontal scroll (in display columns, accounting for CJK width)
    let cursor_line = textarea
        .lines()
        .get(cursor_row)
        .map(|s| s.as_str())
        .unwrap_or("");
    let cursor_display_col: usize = cursor_line
        .chars()
        .take(cursor_col)
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
        .sum();
    let scroll_col = cursor_display_col.saturating_sub(visible_width.saturating_sub(1));
    let visible_col = cursor_display_col.saturating_sub(scroll_col);

    Some((
        textarea_area.x + visible_col as u16,
        textarea_area.y + visible_row as u16,
    ))
}

// ─── nanosleep64 stub for WinLibs POSIX compatibility ───────────────────────

#[cfg(target_os = "windows")]
#[no_mangle]
extern "C" fn nanosleep64(req: *const libc::timespec, rem: *mut libc::timespec) -> i32 {
    extern "system" {
        fn Sleep(dwMilliseconds: u32);
    }
    if req.is_null() {
        return -1;
    }
    let r = unsafe { &*req };
    let ms = (r.tv_sec as u64 * 1000 + r.tv_nsec as u64 / 1_000_000) as u32;
    let ms = if ms == 0 { 1 } else { ms };
    unsafe {
        Sleep(ms);
    }
    if !rem.is_null() {
        unsafe {
            (*rem).tv_sec = 0;
            (*rem).tv_nsec = 0;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_pos_empty_textarea() {
        let ta = TextArea::default();
        // 0-height area should return None
        assert!(textarea_cursor_pos(&ta, Rect::new(0, 0, 80, 0)).is_none());
        // 0-width area should return None
        assert!(textarea_cursor_pos(&ta, Rect::new(0, 0, 0, 24)).is_none());
    }

    #[test]
    fn test_cursor_pos_top_left() {
        let mut ta = TextArea::default();
        ta.insert_str("hello");
        ta.move_cursor(tui_textarea::CursorMove::Jump(0, 0));
        // Cursor at (0, 0), textarea at (5, 10)
        let pos = textarea_cursor_pos(&ta, Rect::new(5, 10, 80, 24));
        assert_eq!(pos, Some((5, 10)));
    }

    #[test]
    fn test_cursor_pos_after_text() {
        let mut ta = TextArea::default();
        ta.insert_str("hi");
        // Cursor at (0, 2) after "hi"
        let pos = textarea_cursor_pos(&ta, Rect::new(0, 0, 80, 24));
        assert_eq!(pos, Some((2, 0)));
    }

    #[test]
    fn test_cursor_pos_with_cjk() {
        let mut ta = TextArea::default();
        ta.insert_str("你好");
        // Cursor at (0, 2 chars) which is display column 4
        let pos = textarea_cursor_pos(&ta, Rect::new(0, 10, 80, 24));
        assert_eq!(pos, Some((4, 10)));
    }

    #[test]
    fn test_cursor_pos_scroll_below_viewport() {
        let mut ta = TextArea::default();
        for _ in 0..30 {
            ta.insert_str("line\n");
        }
        // Cursor at line 30 with 24-row viewport: scroll to show cursor
        // scroll_row = 30 - (24 - 1) = 7, visible_row = 30 - 7 = 23
        let pos = textarea_cursor_pos(&ta, Rect::new(3, 5, 80, 24));
        assert_eq!(pos, Some((3, 5 + 23)));
    }
}
