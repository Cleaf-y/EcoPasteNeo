//! Preview request lifetime is independent of list paging and paste target identity.
use anyhow::Result;
use ecopaste_neo_core::preview::PreviewContent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewRequest {
    generation: u64,
    pub id: String,
}

#[derive(Default)]
pub struct PreviewState {
    pub open: bool,
    pub loading: bool,
    pub id: Option<String>,
    pub content: Option<PreviewContent>,
    pub failed: bool,
    generation: u64,
}

impl PreviewState {
    /// Every open or selection change invalidates the previous display and request.
    pub fn begin(&mut self, id: String) -> PreviewRequest {
        self.generation = self.generation.wrapping_add(1);
        self.open = true;
        self.loading = true;
        self.id = Some(id.clone());
        self.content = None;
        self.failed = false;
        PreviewRequest {
            generation: self.generation,
            id,
        }
    }

    /// A completion may update content only while its exact record is still open.
    pub fn complete(&mut self, request: PreviewRequest, result: Result<PreviewContent>) -> bool {
        if !self.open
            || !self.loading
            || self.generation != request.generation
            || self.id.as_deref() != Some(request.id.as_str())
        {
            return false;
        }
        self.loading = false;
        match result {
            Ok(content) => self.content = Some(content),
            Err(_) => self.failed = true,
        }
        true
    }

    /// Closing also invalidates pending errors; no completion is allowed to reopen a view.
    pub fn close(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.open = false;
        self.loading = false;
        self.id = None;
        self.content = None;
        self.failed = false;
    }
}

/// Split logical lines into bounded render units while retaining Unicode text.
pub fn text_rows(text: &str) -> Vec<String> {
    text.split('\n')
        .flat_map(|line| {
            let characters: Vec<_> = line.trim_end_matches('\r').chars().collect();
            if characters.is_empty() {
                return vec![String::from(" ")];
            }
            characters
                .chunks(128)
                .map(|chunk| chunk.iter().collect())
                .collect()
        })
        .collect()
}

/// Fit an image within a fixed viewport without relying on intrinsic layout dimensions.
pub fn fitted_image_size(width: f32, height: f32, max_width: f32, max_height: f32) -> (f32, f32) {
    let width = width.max(1.0);
    let height = height.max(1.0);
    let scale = (max_width.max(1.0) / width).min(max_height.max(1.0) / height);
    (width * scale, height * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rapid_switches_reject_old_content_and_errors_including_same_id_reopen() {
        let mut state = PreviewState::default();
        let large_image = state.begin("image".into());
        let short_text = state.begin("text".into());
        assert!(!state.complete(large_image.clone(), Ok(PreviewContent::Text("old".into()))));
        assert!(!state.complete(large_image, Err(anyhow::anyhow!("old failure"))));
        assert!(state.loading);
        assert!(!state.failed);
        assert!(state.complete(
            short_text.clone(),
            Ok(PreviewContent::Text("current".into()))
        ));
        assert!(matches!(state.content, Some(PreviewContent::Text(ref text)) if text == "current"));
        state.close();
        let reopened = state.begin("text".into());
        assert!(!state.complete(short_text, Ok(PreviewContent::Text("old same id".into()))));
        assert!(state.complete(reopened, Ok(PreviewContent::Text("reopened".into()))));
    }

    #[test]
    fn closing_during_load_keeps_late_success_and_failure_invisible() {
        let mut state = PreviewState::default();
        let pending = state.begin("record".into());
        state.close();
        assert!(!state.complete(pending.clone(), Ok(PreviewContent::Text("late".into()))));
        assert!(!state.complete(pending, Err(anyhow::anyhow!("late failure"))));
        assert!(!state.open);
        assert!(state.content.is_none());
        assert!(!state.failed);
    }

    #[test]
    fn redaction_reload_clears_original_and_rejects_older_unmasked_result() {
        let mut state = PreviewState::default();
        let exposed = state.begin("sensitive".into());
        state.complete(
            exposed,
            Ok(PreviewContent::Text("synthetic original".into())),
        );
        let pending = state.begin("sensitive".into());
        let masked = state.begin("sensitive".into());
        assert!(state.content.is_none());
        assert!(!state.complete(pending, Ok(PreviewContent::Text("stale original".into()))));
        assert!(state.complete(masked, Ok(PreviewContent::Hidden)));
        assert!(matches!(state.content, Some(PreviewContent::Hidden)));
    }

    #[test]
    fn bounded_rows_keep_unicode_and_the_long_text_tail() {
        let text = format!("{}\n\nEND-OF-PREVIEW", "中文与🌲".repeat(1000));
        let rows = text_rows(&text);
        assert!(rows.iter().all(|row| row.chars().count() <= 128));
        assert_eq!(rows.last().map(String::as_str), Some("END-OF-PREVIEW"));
        assert_eq!(rows[..rows.len() - 2].concat(), "中文与🌲".repeat(1000));
    }
}
