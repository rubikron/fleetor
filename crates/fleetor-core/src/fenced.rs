//! Fenced-block extraction, shared by the transcript-scraped contracts that ride
//! in a worker's final message: the `fleet-report` (see [`crate::report`]) and
//! the `fleet-review` verdict (see [`crate::review`]). One implementation so the
//! two parsers can't drift.

/// Pull the contents of the **last** ```` ```<fence> … ``` ```` block out of
/// `text`. Scanning for the last block means a worker that shows a draft then a
/// final block yields the final one. Returns `None` when no such block exists.
pub(crate) fn extract_last_fenced(text: &str, fence: &str) -> Option<String> {
    let open = format!("```{fence}");
    let mut search_from = 0usize;
    let mut last: Option<String> = None;
    while let Some(rel) = text[search_from..].find(&open) {
        let block_start = search_from + rel + open.len();
        // Skip to the end of the info-string line.
        let after_info = match text[block_start..].find('\n') {
            Some(nl) => block_start + nl + 1,
            None => break,
        };
        // The block ends at the next closing fence.
        let Some(close_rel) = text[after_info..].find("```") else {
            break;
        };
        let body = &text[after_info..after_info + close_rel];
        last = Some(body.trim().to_string());
        search_from = after_info + close_rel + 3;
    }
    last
}
