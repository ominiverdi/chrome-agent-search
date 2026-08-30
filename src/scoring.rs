use crate::SearchResult;

pub(crate) fn assess_quality(results: &[SearchResult], query: &str) -> f64 {
    if results.is_empty() {
        return 0.0;
    }
    let common = [
        "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by",
        "is", "are", "was", "were", "be", "been", "have", "has", "had", "do", "does", "did",
        "will", "would", "could", "should", "may", "might", "must", "can", "site",
    ];
    let normalized_query = query
        .split_whitespace()
        .filter(|term| !term.to_lowercase().starts_with("site:"))
        .collect::<Vec<_>>()
        .join(" ");
    let words: Vec<String> = normalized_query
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() > 2 && !common.contains(w))
        .map(ToString::to_string)
        .collect();
    if words.is_empty() {
        return 0.5;
    }
    let mut total = 0.0;
    let mut total_weight = 0.0;
    for (idx, result) in results.iter().enumerate() {
        let combined = format!("{} {} {}", result.title, result.snippet, result.url).to_lowercase();
        let keyword_matches = words
            .iter()
            .filter(|w| combined.contains(w.as_str()))
            .count();
        let mut phrase_matches = 0;
        for pair in words.windows(2) {
            if combined.contains(&pair.join(" ")) {
                phrase_matches += 1;
            }
        }
        if words.len() >= 3 && combined.contains(&words[..3].join(" ")) {
            phrase_matches += 1;
        }
        let score = (keyword_matches as f64 / words.len() as f64) + (phrase_matches as f64 * 0.3);
        let top_result_boost = if idx == 0 { 0.2 } else { 0.0 };
        let weight = 1.0 / (idx + 1) as f64;
        total += (score + top_result_boost).min(1.0) * weight;
        total_weight += weight;
    }
    total / total_weight
}
