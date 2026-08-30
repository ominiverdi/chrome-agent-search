use base64::Engine as Base64Engine;
use scraper::{Html, Selector};
use url::Url;

use crate::types::SearchResult;

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
        results.push(SearchResult::new(title, url, snippet));
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
        results.push(SearchResult::new(title, url, snippet));
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
            results.push(SearchResult::new(title, url, snippet));
        }
        if results.len() >= limit {
            break;
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duckduckgo_lite_parser_extracts_result_and_decodes_redirect() {
        let html = r#"
        <html><body><table>
          <tr><td>1.</td><td><a class="result-link" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.zdnet.com%2Farticle%2Fgreg%2Dkroah%2Dhartman%2Dlinux%2Dkernel%2Drust%2F&amp;rut=abc">'Rust makes coding fun again' - ZDNET</a></td></tr>
          <tr><td></td><td>At Open Source Summit India, Greg Kroah-Hartman said the Linux kernel is moving toward Rust.</td></tr>
          <tr><td></td><td>www.zdnet.com/article/greg-kroah-hartman-linux-kernel-rust/</td></tr>
        </table></body></html>
        "#;
        let results = parse_duckduckgo_lite_html(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "'Rust makes coding fun again' - ZDNET");
        assert_eq!(
            results[0].url,
            "https://www.zdnet.com/article/greg-kroah-hartman-linux-kernel-rust/"
        );
        assert!(results[0].snippet.contains("Greg Kroah-Hartman"));
    }

    #[test]
    fn bing_parser_prefers_title_link_and_decodes_redirect() {
        let target = "https://thenextweb.com/news/speed-not-selling-point-ai-agents-caution";
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(target.as_bytes());
        let html = format!(
            r#"
            <html><body><ol>
              <li class="b_algo">
                <div><a href="https://www.bing.com/ck/a?u=a1{}">thenextweb.com › news</a></div>
                <h2><a href="https://www.bing.com/ck/a?u=a1{}">Speed is no longer a selling point for AI agents, caution is - TNW</a></h2>
                <div class="b_caption"><p>AI agents need caution more than raw speed.</p></div>
              </li>
            </ol></body></html>
            "#,
            encoded, encoded
        );
        let results = parse_bing_html(&html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].title,
            "Speed is no longer a selling point for AI agents, caution is - TNW"
        );
        assert_eq!(results[0].url, target);
        assert!(results[0].snippet.contains("caution"));
    }

    #[test]
    fn brave_parser_extracts_title_snippet_and_url() {
        let html = r#"
        <html><body>
          <div data-type="web">
            <a href="https://www.zdnet.com/article/greg-kroah-hartman-linux-kernel-rust/">
              <div class="site-name-content"><div>ZDNET</div><cite>zdnet.com › article</cite></div>
              <div class="title search-snippet-title">'Rust makes coding fun again': Why Linux is moving away from C, according to Greg Kroah-Hartman | ZDNET</div>
            </a>
            <div class="generic-snippet"><div class="content">The Linux kernel is moving toward Rust, Greg Kroah-Hartman said.</div></div>
          </div>
        </body></html>
        "#;
        let results = parse_brave_html(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].url,
            "https://www.zdnet.com/article/greg-kroah-hartman-linux-kernel-rust/"
        );
        assert!(
            results[0]
                .title
                .starts_with("'Rust makes coding fun again'")
        );
        assert!(results[0].snippet.contains("Linux kernel"));
    }
}
