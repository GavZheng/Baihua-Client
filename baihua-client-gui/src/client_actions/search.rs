//! In-room search: input-box `#` search and the standalone panel queries.

use super::*;

impl Client {
    // ==================== Search ====================

    /// Whether in search mode (input box starts with #)
    pub fn in_search_mode(&self) -> bool {
        self.draft.trim_start().starts_with('#')
    }

    /// Search keyword
    pub fn search_keyword(&self) -> String {
        let trimmed = self.draft.trim_start();
        trimmed
            .strip_prefix('#')
            .map(|rest| rest.trim_start().to_string())
            .unwrap_or_default()
    }

    /// Execute search: look in loaded messages; clear results when keyword is empty
    pub fn run_search(&mut self, keyword: &str) {
        if keyword.trim().is_empty() {
            self.search_result = None;
            return;
        }
        let matches = self.messages_matching(keyword);
        if matches.is_empty() {
            self.search_result = Some((keyword.to_string(), Vec::new(), 0));
            return;
        }
        let last = matches.len() - 1;
        self.pending_scroll_message_id = Some(matches[last].clone());
        self.search_result = Some((keyword.to_string(), matches, last));
    }

    /// Look for keyword in loaded messages (shared by the input-box search and the standalone panel).
    pub fn messages_matching(&self, keyword: &str) -> Vec<String> {
        let needle = keyword.to_lowercase();
        self.messages
            .iter()
            .filter(|message| message.content.to_lowercase().contains(&needle))
            .map(|message| message.id.clone())
            .collect()
    }

    /// Jump to previous/next match
    pub fn navigate_search(&mut self, backwards: bool) {
        let Some((keyword, matches, index)) = self.search_result.clone() else {
            return;
        };
        if matches.is_empty() {
            return;
        }
        let next = if backwards {
            (index + matches.len() - 1) % matches.len()
        } else {
            (index + 1) % matches.len()
        };
        self.pending_scroll_message_id = Some(matches[next].clone());
        self.search_result = Some((keyword, matches, next));
    }

    /// Set of message IDs matching the keyword (the interface draws highlight backgrounds based on this)
    pub fn search_matches(&self) -> (Vec<String>, Option<String>) {
        match &self.search_result {
            Some((_keyword, matches, index)) => (matches.clone(), matches.get(*index).cloned()),
            None => (Vec::new(), None),
        }
    }

    /// The keyword used by the last executed search (the title displays it, not what's being typed in the input box).
    /// the text in the input box might have been changed but not submitted; displaying it directly would mismatch the title and matches.
    pub fn searched_keyword(&self) -> Option<&str> {
        self.search_result
            .as_ref()
            .map(|(keyword, _matches, _index)| keyword.as_str())
    }

    /// The panel's Enter-committed search: store the keyword with the loaded messages
    /// that match it, empty clears it. Unlike `run_search` it never scrolls.
    pub fn run_panel_search(&mut self, keyword: &str) {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            self.panel_search_result = None;
            return;
        }
        self.panel_search_result = Some((keyword.to_string(), self.messages_matching(keyword)));
    }

    /// IDs the search panel lists for the typed keyword: quick search rescans live;
    /// otherwise only the Enter-committed result while the keyword still matches.
    pub fn panel_match_ids(&self, keyword: &str) -> Vec<String> {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            return Vec::new();
        }
        if self.quick_search {
            return self.messages_matching(keyword);
        }
        match &self.panel_search_result {
            Some((searched, matches)) if searched == keyword => matches.clone(),
            _ => Vec::new(),
        }
    }

    /// Input box change: leaving search mode clears the results, quick search rescans
    /// per keystroke, and a plain search drops stale results at once.
    pub fn handle_draft_changed(&mut self) {
        if !self.in_search_mode() {
            self.search_result = None;
            return;
        }
        if self.quick_search {
            let keyword = self.search_keyword();
            if keyword.is_empty() {
                self.search_result = None;
                return;
            }
            self.run_search(&keyword);
            return;
        }
        if let Some(searched) = self.searched_keyword()
            && searched != self.search_keyword()
        {
            self.search_result = None;
        }
    }
}
