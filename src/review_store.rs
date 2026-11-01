use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::app::ReviewComment;

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

    pub fn load(&self) -> Result<Vec<ReviewComment>, String> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let contents = fs::read_to_string(&self.path)
            .map_err(|error| format!("Cannot read saved reviews: {error}"))?;
        serde_json::from_str(&contents).map_err(|error| format!("Invalid saved reviews: {error}"))
    }

    pub fn save(&self, comments: &[ReviewComment]) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "Review store has no parent directory".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Cannot create review store: {error}"))?;
        let temporary = temporary_path(&self.path);
        let contents = serde_json::to_vec_pretty(comments)
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
    use crate::app::{ReviewScope, ReviewThreadMessage, ReviewThreadRole};

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

        store.save(&comments).unwrap();
        assert_eq!(store.load().unwrap(), comments);

        fs::remove_dir_all(root).unwrap();
    }
}
