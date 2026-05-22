use std::fmt;

/// Parsed conventional commit type
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CommitType {
    Feat,
    Fix,
    Refactor,
    Chore,
    Docs,
    Test,
    Other,
}

impl CommitType {
    pub fn from_subject(subject: &str) -> Self {
        // Match "type(scope): ..." or "type: ..."
        let re = regex::Regex::new(r"^(\w+)(\([^)]*\))?!?:\s").unwrap();
        if let Some(caps) = re.captures(subject) {
            match caps.get(1).unwrap().as_str() {
                "feat" => CommitType::Feat,
                "fix" => CommitType::Fix,
                "refactor" => CommitType::Refactor,
                "chore" => CommitType::Chore,
                "docs" => CommitType::Docs,
                "test" => CommitType::Test,
                _ => CommitType::Other,
            }
        } else {
            CommitType::Other
        }
    }
}

impl fmt::Display for CommitType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommitType::Feat => write!(f, "feat"),
            CommitType::Fix => write!(f, "fix"),
            CommitType::Refactor => write!(f, "refactor"),
            CommitType::Chore => write!(f, "chore"),
            CommitType::Docs => write!(f, "docs"),
            CommitType::Test => write!(f, "test"),
            CommitType::Other => write!(f, "other"),
        }
    }
}

/// A single file change in a commit
#[derive(Debug, Clone)]
pub struct FileChange {
    pub added: u64,
    pub deleted: u64,
}

/// Co-author extracted from commit message
#[derive(Debug, Clone)]
pub struct CoAuthor {
    pub name: String,
    pub email: String,
}

/// A parsed commit with all relevant fields
#[derive(Debug, Clone)]
pub struct ParsedCommit {
    pub hash: String,
    pub author_name: String,
    pub author_email: String,
    pub subject: String,
    pub commit_type: CommitType,
    pub co_authors: Vec<CoAuthor>,
    pub files: Vec<FileChange>,
}

/// Extract all Co-Authored-By lines from commit body.
/// Matches: "Co-Authored-By: Name <email>" (case-insensitive)
pub fn parse_co_authors(body: &str) -> Vec<CoAuthor> {
    let re = regex::Regex::new(r"(?i)^Co-Authored-By:\s*(.+?)\s*<([^>]+)>").unwrap();
    body.lines()
        .filter_map(|line| {
            re.captures(line).map(|caps| CoAuthor {
                name: caps.get(1).unwrap().as_str().trim().to_string(),
                email: caps.get(2).unwrap().as_str().trim().to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit_type_feat() {
        assert_eq!(
            CommitType::from_subject("feat(auth): add login"),
            CommitType::Feat
        );
        assert_eq!(CommitType::from_subject("feat: add login"), CommitType::Feat);
    }

    #[test]
    fn test_commit_type_fix() {
        assert_eq!(
            CommitType::from_subject("fix: crash on null"),
            CommitType::Fix
        );
    }

    #[test]
    fn test_commit_type_other() {
        assert_eq!(
            CommitType::from_subject("random message without type"),
            CommitType::Other
        );
        assert_eq!(CommitType::from_subject("WIP: stuff"), CommitType::Other);
    }

    #[test]
    fn test_parse_co_authors_single() {
        let body = "Some description\n\nCo-Authored-By: Alice <alice@corp.com>";
        let result = parse_co_authors(body);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Alice");
        assert_eq!(result[0].email, "alice@corp.com");
    }

    #[test]
    fn test_parse_co_authors_multiple() {
        let body =
            "Co-Authored-By: Alice <alice@corp.com>\nCo-authored-by: Bob <bob@corp.com>";
        let result = parse_co_authors(body);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_parse_co_authors_empty() {
        let body = "Just a normal commit\nNo co-authors here";
        let result = parse_co_authors(body);
        assert_eq!(result.len(), 0);
    }
}
