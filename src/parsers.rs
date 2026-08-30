use base64::Engine as Base64Engine;
use scraper::{Html, Selector};
use url::Url;

use crate::SearchResult;

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("valid selector")
}

fn clean_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn element_text(element: scraper::ElementRef<'_>) -> String {
    clean_text(&element.text().collect::<Vec<_>>().join(" "))
}

pub(crate) fn clean_duckduckgo_url(href: &str) -> String {
    let parsed = Url::parse(href)
        .or_else(|_| Url::parse("https://duckduckgo.com").and_then(|base| base.join(href)));
    parsed
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "uddg")
                .map(|(_, value)| value.into_owned())
        })
        .unwrap_or_else(|| href.to_string())
}

pub(crate) fn clean_bing_url(href: &str) -> String {
    let Ok(url) = Url::parse(href) else {
        return href.to_string();
    };
    if !url.host_str().unwrap_or_default().contains("bing.com") {
        return href.to_string();
    }
    let Some(mut encoded) = url
        .query_pairs()
        .find(|(key, _)| key == "u")
        .map(|(_, value)| value.into_owned())
    else {
        return href.to_string();
    };
    if let Some(stripped) = encoded.strip_prefix("a1") {
        encoded = stripped.to_string();
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded.as_bytes())
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(encoded.as_bytes()))
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_else(|| href.to_string())
}

pub(crate) fn parse_duckduckgo_lite_html(html: &str, limit: usize) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let link_sel = selector("a.result-link, .result__title a, a.result__a");
    let tr_sel = selector("tr");
    let rows: Vec<_> = doc.select(&tr_sel).collect();
    let mut results = Vec::new();
    for (idx, row) in rows.iter().enumerate() {
        let Some(a) = row.select(&link_sel).next() else {
            continue;
        };
        let title = element_text(a);
        let href = a.value().attr("href").unwrap_or_default();
        let url = clean_duckduckgo_url(href);
        if title.is_empty() || url.contains("duckduckgo.com") {
            continue;
        }
        let mut snippet = String::new();
        for next_row in rows.iter().skip(idx + 1).take(3) {
            let text = element_text(*next_row);
            if !text.is_empty() && !text.starts_with("http") && !text.contains("duckduckgo.com") {
                snippet = text;
                break;
            }
        }
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
        if results.len() >= limit {
            break;
        }
    }
    results
}

pub(crate) fn parse_bing_html(html: &str, limit: usize) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let item_sel = selector("li.b_algo, .b_result, .b_card");
    let primary_link_sel = selector("h2 a, .b_title a");
    let fallback_link_sel = selector("a[data-seid], a[href]");
    let snippet_sel = selector(
        ".b_caption p, .b_snippet, .b_descript, .b_caption, .b_excerpt, p, .b_algo_content",
    );
    let mut results = Vec::new();
    for item in doc.select(&item_sel) {
        let Some(a) = item
            .select(&primary_link_sel)
            .find(|a| !element_text(*a).is_empty())
            .or_else(|| {
                item.select(&fallback_link_sel)
                    .find(|a| !element_text(*a).is_empty())
            })
        else {
            continue;
        };
        let title = element_text(a);
        let url = clean_bing_url(a.value().attr("href").unwrap_or_default());
        if title.is_empty() || url.contains("bing.com") {
            continue;
        }
        let snippet = item
            .select(&snippet_sel)
            .map(element_text)
            .find(|text| !text.is_empty())
            .unwrap_or_default();
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
        if results.len() >= limit {
            break;
        }
    }
    results
}

pub(crate) fn parse_brave_html(html: &str, limit: usize) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let item_sel = selector("[data-type=web]");
    let link_sel = selector("a[href]");
    let title_sel = selector(".title, .search-snippet-title");
    let snippet_sel = selector(".generic-snippet .content, .snippet-content, .description, p");
    let mut results = Vec::new();
    for item in doc.select(&item_sel) {
        let lines: Vec<String> = element_text(item)
            .split('\n')
            .map(clean_text)
            .filter(|line| !line.is_empty())
            .collect();
        let Some(a) = item.select(&link_sel).find(|a| {
            a.value()
                .attr("href")
                .unwrap_or_default()
                .starts_with("http")
        }) else {
            continue;
        };
        let url = a.value().attr("href").unwrap_or_default().to_string();
        if url.contains("search.brave.com") {
            continue;
        }
        let link_text = element_text(a);
        let title = item
            .select(&title_sel)
            .map(element_text)
            .find(|text| !text.is_empty())
            .or_else(|| {
                lines
                    .iter()
                    .enumerate()
                    .find(|(idx, line)| *idx > 1 && line.len() > 8 && !line.contains(" › "))
                    .map(|(_, line)| line.clone())
            })
            .unwrap_or(link_text);
        let snippet = item
            .select(&snippet_sel)
            .map(element_text)
            .find(|text| !text.is_empty())
            .or_else(|| {
                lines
                    .iter()
                    .enumerate()
                    .find(|(idx, line)| *idx > 2 && **line != title && line.len() > 40)
                    .map(|(_, line)| line.clone())
            })
            .unwrap_or_default();
        if !title.is_empty() {
            results.push(SearchResult {
                title,
                url,
                snippet,
            });
        }
        if results.len() >= limit {
            break;
        }
    }
    results
}
