use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};

use crate::app::{ReviewComment, UnderstandingThread};

#[derive(Default, Serialize, Deserialize)]
pub struct ReviewData {
    pub comments: Vec<ReviewComment>,
    #[serde(default)]
    pub understanding_threads: Vec<UnderstandingThread>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredReviewData {
    Current(ReviewData),
    Legacy(Vec<ReviewComment>),
}

pub struct ReviewStore {
    path: PathBuf,
}

impl ReviewStore {
    pub fn discover() -> Result<Self, String> {
        let output = Command::new("git")
            .args([
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "atig/reviews.json",
            ])
            .output()
            .map_err(|error| format!("Cannot locate Git review store: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        Ok(Self { path })
    }

    pub fn load(&self) -> Result<ReviewData, String> {
        if !self.path.exists() {
            return Ok(ReviewData::default());
        }
        let contents = fs::read_to_string(&self.path)
            .map_err(|error| format!("Cannot read saved reviews: {error}"))?;
        let stored: StoredReviewData = serde_json::from_str(&contents)
            .map_err(|error| format!("Invalid saved reviews: {error}"))?;
        Ok(match stored {
            StoredReviewData::Current(data) => data,
            StoredReviewData::Legacy(comments) => ReviewData {
                comments,
                understanding_threads: Vec::new(),
            },
        })
    }

    pub fn save(
        &self,
        comments: &[ReviewComment],
        understanding_threads: &[UnderstandingThread],
    ) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "Review store has no parent directory".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Cannot create review store: {error}"))?;
        let temporary = temporary_path(&self.path);
        let contents = serde_json::to_vec_pretty(&ReviewData {
            comments: comments.to_vec(),
            understanding_threads: understanding_threads.to_vec(),
        })
        .map_err(|error| format!("Cannot encode reviews: {error}"))?;
        fs::write(&temporary, contents)
            .map_err(|error| format!("Cannot write saved reviews: {error}"))?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| format!("Cannot replace saved reviews: {error}"))
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension("json.tmp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{ReviewScope, ReviewThreadMessage, ReviewThreadRole, UnderstandingThread};

    #[test]
    fn round_trips_review_threads() {
        let root = std::env::temp_dir().join(format!("atig-review-store-{}", std::process::id()));
        let store = ReviewStore {
            path: root.join("reviews.json"),
        };
        let comments = vec![ReviewComment {
            commit: "abc123".into(),
            scope: ReviewScope::File("src/main.rs".into()),
            diff_index: 4,
            body: "Simplify this file".into(),
            context: Vec::new(),
            thread: vec![ReviewThreadMessage {
                role: ReviewThreadRole::Agent("codex:session".into()),
                body: "Done".into(),
            }],
        }];

        let understanding_threads = vec![UnderstandingThread {
            source: "WORKTREE".into(),
            version: "WORKTREE".into(),
            file: "src/main.rs".into(),
            line: 8,
            end_line: None,
            question: "Why?".into(),
            agent: "codex:session".into(),
            answer: Some("Because.".into()),
            references: Vec::new(),
        }];

        store.save(&comments, &understanding_threads).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.comments, comments);
        assert_eq!(loaded.understanding_threads, understanding_threads);

        let legacy_thread = r#"{"comments":[],"understanding_threads":[{"source":"WORKTREE","version":"WORKTREE","file":"src/main.rs","line":8,"question":"Why?","agent":"codex:session","answer":null,"references":[]}]}"#;
        fs::write(&store.path, legacy_thread).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.understanding_threads[0].end_line, None);

        fs::write(&store.path, serde_json::to_vec(&comments).unwrap()).unwrap();
        let legacy = store.load().unwrap();
        assert_eq!(legacy.comments, comments);
        assert!(legacy.understanding_threads.is_empty());

        fs::remove_dir_all(root).unwrap();
    }
}
