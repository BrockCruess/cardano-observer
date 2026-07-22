//! List pagination: `count` / `page` / `order` query params plus the
//! `unpaged: true` request header that disables paging entirely.

use crate::error::ApiError;
use axum::http::HeaderMap;
use serde::Deserialize;

pub const MAX_COUNT: i64 = 100;
pub const MAX_PAGE: i64 = 2_147_483_646;

#[derive(Debug, Default, Deserialize)]
pub struct PageParams {
    pub count: Option<i64>,
    pub page: Option<i64>,
    pub order: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Asc,
    Desc,
}

impl Order {
    /// SQL keyword for interpolation into ORDER BY clauses. Only ever one of
    /// two fixed tokens, so it is safe to format into a query string.
    pub fn sql(&self) -> &'static str {
        match self {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        }
    }
}

#[derive(Debug)]
pub struct Page {
    /// `None` disables the LIMIT (unpaged listing).
    pub limit: Option<i64>,
    pub offset: i64,
    pub order: Order,
}

impl Page {
    pub fn resolve(params: &PageParams, headers: &HeaderMap) -> Result<Page, ApiError> {
        let order = match params.order.as_deref() {
            None | Some("asc") => Order::Asc,
            Some("desc") => Order::Desc,
            Some(other) => {
                return Err(ApiError::bad_request(format!(
                    "querystring/order must be equal to one of the allowed values: asc, desc (got {other})"
                )));
            }
        };
        if headers.contains_key("unpaged") {
            return Ok(Page {
                limit: None,
                offset: 0,
                order,
            });
        }
        let count = match params.count {
            None => MAX_COUNT,
            Some(c) if (1..=MAX_COUNT).contains(&c) => c,
            Some(_) => {
                return Err(ApiError::bad_request(
                    "querystring/count must be an integer between 1 and 100",
                ));
            }
        };
        let page = match params.page {
            None => 1,
            Some(p) if (1..=MAX_PAGE).contains(&p) => p,
            Some(_) => {
                return Err(ApiError::bad_request(
                    "querystring/page must be a positive integer",
                ));
            }
        };
        Ok(Page {
            limit: Some(count),
            offset: (page - 1) * count,
            order,
        })
    }
}

/// Parses an optional `true` / `false` query value, used by boolean filters.
pub fn parse_bool_filter(value: Option<&str>, name: &str) -> Result<Option<bool>, ApiError> {
    match value {
        None => Ok(None),
        Some("true") => Ok(Some(true)),
        Some("false") => Ok(Some(false)),
        Some(other) => Err(ApiError::bad_request(format!(
            "querystring/{name} must be equal to one of the allowed values: true, false (got {other})"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(count: Option<i64>, page: Option<i64>, order: Option<&str>) -> PageParams {
        PageParams {
            count,
            page,
            order: order.map(str::to_string),
        }
    }

    #[test]
    fn defaults() {
        let p = Page::resolve(&params(None, None, None), &HeaderMap::new()).unwrap();
        assert_eq!(p.limit, Some(100));
        assert_eq!(p.offset, 0);
        assert_eq!(p.order, Order::Asc);
    }

    #[test]
    fn paging_math() {
        let p = Page::resolve(&params(Some(25), Some(3), Some("desc")), &HeaderMap::new()).unwrap();
        assert_eq!(p.limit, Some(25));
        assert_eq!(p.offset, 50);
        assert_eq!(p.order, Order::Desc);
    }

    #[test]
    fn rejects_out_of_range() {
        assert!(Page::resolve(&params(Some(0), None, None), &HeaderMap::new()).is_err());
        assert!(Page::resolve(&params(Some(101), None, None), &HeaderMap::new()).is_err());
        assert!(Page::resolve(&params(None, Some(0), None), &HeaderMap::new()).is_err());
        assert!(Page::resolve(&params(None, None, Some("sideways")), &HeaderMap::new()).is_err());
    }

    #[test]
    fn unpaged_header_disables_limit() {
        let mut headers = HeaderMap::new();
        headers.insert("unpaged", "true".parse().unwrap());
        let p = Page::resolve(&params(Some(10), Some(5), None), &headers).unwrap();
        assert_eq!(p.limit, None);
        assert_eq!(p.offset, 0);
    }

    #[test]
    fn bool_filter() {
        assert_eq!(parse_bool_filter(None, "retired").unwrap(), None);
        assert_eq!(parse_bool_filter(Some("true"), "retired").unwrap(), Some(true));
        assert_eq!(parse_bool_filter(Some("false"), "retired").unwrap(), Some(false));
        assert!(parse_bool_filter(Some("nope"), "retired").is_err());
    }
}
