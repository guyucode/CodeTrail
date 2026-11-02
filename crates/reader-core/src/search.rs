//! Bounded literal text search; never executes project tools or rebuilds syntax indexes.
use crate::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeMatch {
    pub path: String,
    pub line: u32,
    pub preview: String,
    pub file_hash: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub query: String,
    pub matches: Vec<CodeMatch>,
    pub warnings: Vec<String>,
    pub truncated: bool,
}

pub fn search_code(root: &Path, query: &str) -> Result<SearchResult> {
    let root = canonical_root(root)?;
    let query = query.trim();
    ensure!(
        !query.is_empty() && query.chars().count() <= 200 && !query.chars().any(char::is_control),
        "请输入1至200个字符的单行关键词"
    );
    let needle = query.to_lowercase();
    let mut result = SearchResult {
        query: query.into(),
        matches: Vec::new(),
        warnings: Vec::new(),
        truncated: false,
    };
    let mut paths = Vec::new();
    for entry in source_walker(&root).build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                result.truncated = true;
                continue;
            }
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if paths.len() == MAX_FILES {
            result.truncated = true;
            break;
        }
        paths.push(
            entry
                .path()
                .strip_prefix(&root)?
                .to_string_lossy()
                .replace('\\', "/"),
        );
    }
    paths.sort();
    let mut bytes = 0u64;
    for path in paths {
        let content = match safe_source_path(&root, &path)
            .and_then(|p| read_limited_text(&p, MAX_FILE_BYTES))
        {
            Ok(text) => text,
            Err(e) => {
                if !e.to_string().contains("二进制") {
                    result.truncated = true;
                }
                continue;
            }
        };
        bytes += content.len() as u64;
        if bytes > MAX_INDEX_BYTES {
            result.truncated = true;
            break;
        }
        let mut hash = None;
        for (i, line) in content.lines().enumerate() {
            if !line.to_lowercase().contains(&needle) {
                continue;
            }
            if result.matches.len() == 200 {
                result.truncated = true;
                break;
            }
            result.matches.push(CodeMatch {
                path: path.clone(),
                line: i as u32 + 1,
                preview: line.trim().chars().take(240).collect(),
                file_hash: hash
                    .get_or_insert_with(|| digest(content.as_bytes()))
                    .clone(),
            });
        }
        if result.matches.len() == 200 {
            result.truncated = true;
            break;
        }
    }
    if result.truncated {
        result.warnings.push("搜索范围受忽略规则、10,000文件 / 64 MiB / 单文件2 MiB及200条结果限制，部分文件可能无法读取；未命中不代表代码中不存在。".into());
    }
    Ok(result)
}
