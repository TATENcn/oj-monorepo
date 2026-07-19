use std::sync::Arc;

use hyper::Uri;

use crate::config::{MatchType, RouteConfig};

#[derive(Debug, Clone)]
pub struct RouteMatch {
    pub config: Arc<RouteConfig>,
}

pub fn match_route(routes: &[Arc<RouteConfig>], request_path: &str) -> Option<RouteMatch> {
    routes
        .iter()
        .fold(None, |best: Option<((usize, bool), &Arc<RouteConfig>)>, route| {
            let matches = match route.match_type {
                MatchType::Exact => request_path == route.path,
                MatchType::Prefix => is_prefix_match(&route.path, request_path),
            };
            if !matches {
                return best;
            }
            let is_exact = matches!(route.match_type, MatchType::Exact);
            let key = (route.path.len(), is_exact);
            match best {
                None => Some((key, route)),
                Some((best_key, _)) if key > best_key => Some((key, route)),
                Some(prev) => Some(prev),
            }
        })
        .map(|(_, route)| RouteMatch { config: Arc::clone(route) })
}

fn is_prefix_match(prefix: &str, path: &str) -> bool {
    if !path.starts_with(prefix) {
        return false;
    }

    // Must match a path segment boundary, either exact match or followed by '/'
    path.len() == prefix.len() || path.as_bytes()[prefix.len()] == b'/'
}

pub fn build_upstream_uri(upstream: &str, path: &str, query: Option<&str>) -> Result<Uri, hyper::http::uri::InvalidUri> {
    // Remove trailing slash from upstream uri
    let mut uri = upstream.trim_end_matches('/').to_string();
    uri.push_str(path);

    if let Some(q) = query {
        uri.push('?');
        uri.push_str(q);
    }

    uri.parse()
}
