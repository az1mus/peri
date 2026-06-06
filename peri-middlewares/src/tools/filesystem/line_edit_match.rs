//! 5 级匹配引擎
//! 在文件内容中定位 hunk 的 context 行，从精确到宽松逐级回退。

use super::line_edit_diff::{DiffLine, Hunk};

/// 匹配结果
#[derive(Debug, Clone)]
pub struct MatchResult {
    /// 匹配到的起始行索引（0-based）
    pub line_idx: usize,
    /// 使用的匹配级别
    pub level: MatchLevel,
}

/// 匹配级别
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchLevel {
    L1Exact,
    L2Whitespace,
    L3Similarity(f64),
    L4Anchor,
    L5LineNumber,
}

impl std::fmt::Display for MatchLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchLevel::L1Exact => write!(f, "L1-exact"),
            MatchLevel::L2Whitespace => write!(f, "L2-whitespace"),
            MatchLevel::L3Similarity(r) => write!(f, "L3-similarity({:.2})", r),
            MatchLevel::L4Anchor => write!(f, "L4-anchor"),
            MatchLevel::L5LineNumber => write!(f, "L5-linenumber"),
        }
    }
}

/// 匹配错误
#[derive(Debug)]
pub enum MatchError {
    NotFound { searched_content: String },
    MultipleLocations { positions: Vec<usize> },
}

/// 从 hunk 中提取 old 端的文本行（context + remove 行）
pub fn extract_old_lines(hunk: &Hunk) -> Vec<String> {
    hunk.lines
        .iter()
        .filter_map(|dl| match dl {
            DiffLine::Context(s) => Some(s.clone()),
            DiffLine::Remove(s) => Some(s.clone()),
            DiffLine::Add(_) => None,
        })
        .collect()
}

/// 从 hunk 中提取纯 context 行（不含 remove/add）
pub fn extract_context_lines(hunk: &Hunk) -> Vec<String> {
    hunk.lines
        .iter()
        .filter_map(|dl| match dl {
            DiffLine::Context(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// 在文件行中匹配 hunk，返回匹配位置
pub fn match_hunk(file_lines: &[String], hunk: &Hunk) -> Result<MatchResult, MatchError> {
    let old_lines = extract_old_lines(hunk);

    // L1: 精确匹配
    if let Some(pos) = find_exact(file_lines, &old_lines) {
        return Ok(MatchResult {
            line_idx: pos,
            level: MatchLevel::L1Exact,
        });
    }

    // L2: 空白归一化匹配
    if let Some(pos) = find_whitespace_normalized(file_lines, &old_lines) {
        return Ok(MatchResult {
            line_idx: pos,
            level: MatchLevel::L2Whitespace,
        });
    }

    // L3: 行级相似度匹配
    if let Some((pos, ratio)) = find_by_similarity(file_lines, &old_lines, 0.6) {
        return Ok(MatchResult {
            line_idx: pos,
            level: MatchLevel::L3Similarity(ratio),
        });
    }

    // L4: 关键行锚定（首尾 context 行）
    let ctx_lines = extract_context_lines(hunk);
    if ctx_lines.len() >= 2 {
        if let Some(pos) = find_by_anchor(file_lines, &ctx_lines) {
            return Ok(MatchResult {
                line_idx: pos,
                level: MatchLevel::L4Anchor,
            });
        }
    }

    // L5: 行号兜底
    if hunk.header.old_start > 0 && hunk.header.old_start <= file_lines.len() {
        return Ok(MatchResult {
            line_idx: hunk.header.old_start - 1,
            level: MatchLevel::L5LineNumber,
        });
    }

    Err(MatchError::NotFound {
        searched_content: old_lines
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
    })
}

/// L1: 精确匹配——全文搜索完全相同的连续行，仅单位置时返回
fn find_exact(haystack: &[String], needle: &[String]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let mut positions = Vec::new();
    for i in 0..=haystack.len() - needle.len() {
        if haystack[i..i + needle.len()] == needle[..] {
            positions.push(i);
        }
    }
    if positions.len() == 1 {
        Some(positions[0])
    } else {
        None
    }
}

/// L2: 空白归一化匹配——tab→4spaces + trim 尾部空白
fn find_whitespace_normalized(haystack: &[String], needle: &[String]) -> Option<usize> {
    let normalize = |s: &str| s.replace('\t', "    ").trim_end().to_string();
    let norm_haystack: Vec<String> = haystack.iter().map(|s| normalize(s)).collect();
    let norm_needle: Vec<String> = needle.iter().map(|s| normalize(s)).collect();
    find_exact(&norm_haystack, &norm_needle)
}

/// L3: 行级相似度匹配——用 similar crate 计算 ratio
fn find_by_similarity(
    haystack: &[String],
    needle: &[String],
    threshold: f64,
) -> Option<(usize, f64)> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }

    let needle_text = needle.join("\n");
    let mut best: Option<(usize, f64)> = None;

    for i in 0..=haystack.len().saturating_sub(needle.len()) {
        let candidate: String = haystack[i..i + needle.len()].join("\n");
        let diff = similar::TextDiff::from_lines(&needle_text, &candidate);
        let ratio = diff.ratio() as f64;
        if ratio >= threshold {
            if let Some((_, best_ratio)) = best {
                if ratio > best_ratio {
                    best = Some((i, ratio));
                }
            } else {
                best = Some((i, ratio));
            }
        }
    }

    best
}

/// L4: 关键行锚定——首行和末行 context 必须匹配
fn find_by_anchor(haystack: &[String], ctx_lines: &[String]) -> Option<usize> {
    let first_line = &ctx_lines[0];
    let last_line = &ctx_lines[ctx_lines.len() - 1];
    let span = ctx_lines.len().saturating_sub(1);

    let mut positions = Vec::new();
    for i in 0..haystack.len() {
        if haystack[i].trim_end() == first_line.trim_end() {
            let expected_last = i + span;
            if expected_last < haystack.len()
                && haystack[expected_last].trim_end() == last_line.trim_end()
            {
                positions.push(i);
            }
        }
    }

    if positions.len() == 1 {
        Some(positions[0])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::filesystem::line_edit_diff::HunkHeader;

    fn make_hunk(old_start: usize, lines: Vec<DiffLine>) -> Hunk {
        Hunk {
            header: HunkHeader {
                old_start,
                old_count: lines.len(),
                new_start: old_start,
                new_count: lines.len(),
            },
            lines,
        }
    }

    fn s(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_l1_精确匹配() {
        let file = s(&["aaa", "bbb", "ccc", "ddd", "eee"]);
        let hunk = make_hunk(
            2,
            vec![
                DiffLine::Context("bbb".into()),
                DiffLine::Remove("ccc".into()),
                DiffLine::Add("CCC".into()),
                DiffLine::Context("ddd".into()),
            ],
        );
        let result = match_hunk(&file, &hunk).unwrap();
        assert_eq!(result.line_idx, 1);
        assert_eq!(result.level, MatchLevel::L1Exact);
    }

    #[test]
    fn test_l2_空白归一化() {
        let file = s(&["aaa", "bbb\t", "ccc", "ddd"]);
        let hunk = make_hunk(
            2,
            vec![
                DiffLine::Context("bbb".into()),
                DiffLine::Remove("ccc".into()),
                DiffLine::Add("CCC".into()),
                DiffLine::Context("ddd".into()),
            ],
        );
        let result = match_hunk(&file, &hunk).unwrap();
        assert_eq!(result.level, MatchLevel::L2Whitespace);
    }

    #[test]
    fn test_l3_相似度匹配() {
        // old_lines 包含 context+remove，与文件有微小差异 → L1/L2 失败 → L3 成功
        let file = s(&[
            "fn process(input: &str) -> Result<(), Error> {",
            "    let config = parse(input)?;",
            "    execute(config)",
            "}",
        ]);
        let hunk = make_hunk(
            1,
            vec![
                // context 行与文件有微小差异（多了空格），L1/L2 不匹配
                DiffLine::Context("fn process(input: &str)  -> Result<(), Error> {".into()),
                DiffLine::Remove("    let config = parse(input)?;".into()),
                DiffLine::Add("    let opts = parse(input)?;".into()),
                DiffLine::Context("    execute(config)".into()),
                DiffLine::Context("}".into()),
            ],
        );
        let result = match_hunk(&file, &hunk).unwrap();
        assert!(
            matches!(result.level, MatchLevel::L3Similarity(_)),
            "expected L3, got {:?}",
            result.level
        );
    }

    #[test]
    fn test_l5_行号兜底() {
        let file = s(&["aaa", "bbb", "ccc"]);
        let hunk = make_hunk(
            2,
            vec![
                DiffLine::Context("xxx".into()),
                DiffLine::Remove("yyy".into()),
                DiffLine::Add("YYY".into()),
            ],
        );
        let result = match_hunk(&file, &hunk).unwrap();
        assert_eq!(result.level, MatchLevel::L5LineNumber);
        assert_eq!(result.line_idx, 1);
    }

    #[test]
    fn test_匹配失败() {
        let file = s(&["aaa", "bbb"]);
        let hunk = make_hunk(99, vec![DiffLine::Context("xxx".into())]);
        let result = match_hunk(&file, &hunk);
        assert!(matches!(result, Err(MatchError::NotFound { .. })));
    }
}
