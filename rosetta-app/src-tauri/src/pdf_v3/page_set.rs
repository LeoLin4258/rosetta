use std::{fmt, ops::RangeInclusive};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct PageSet {
    pages: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PageSetError {
    InvalidPage(String),
    InvalidRange(String),
    OutOfBounds { page: u32, page_count: u32 },
}

impl fmt::Display for PageSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPage(value) => write!(formatter, "invalid PDF page: {value}"),
            Self::InvalidRange(value) => write!(formatter, "invalid PDF page range: {value}"),
            Self::OutOfBounds { page, page_count } => {
                write!(formatter, "PDF page {page} is outside 1..={page_count}")
            }
        }
    }
}

impl std::error::Error for PageSetError {}

impl PageSet {
    pub(crate) fn empty() -> Self {
        Self { pages: Vec::new() }
    }

    pub(crate) fn all(page_count: u32) -> Result<Self, PageSetError> {
        if page_count == 0 {
            return Err(PageSetError::InvalidPage(
                "page count must be positive".to_string(),
            ));
        }
        Ok(Self {
            pages: (1..=page_count).collect(),
        })
    }

    pub(crate) fn from_pages<I>(pages: I) -> Result<Self, PageSetError>
    where
        I: IntoIterator<Item = u32>,
    {
        let mut pages = pages.into_iter().collect::<Vec<_>>();
        if let Some(page) = pages.iter().copied().find(|page| *page == 0) {
            return Err(PageSetError::InvalidPage(page.to_string()));
        }
        pages.sort_unstable();
        pages.dedup();
        Ok(Self { pages })
    }

    pub(crate) fn parse(value: &str, page_count: u32) -> Result<Self, PageSetError> {
        if page_count == 0 && !value.trim().is_empty() {
            return Err(PageSetError::InvalidPage(
                "page count must be positive".to_string(),
            ));
        }

        let mut pages = Vec::new();
        for part in value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            if let Some((start, end)) = part.split_once('-') {
                let start = parse_page_number(start, part)?;
                let end = parse_page_number(end, part)?;
                if start > end {
                    return Err(PageSetError::InvalidRange(part.to_string()));
                }
                validate_range(start..=end, page_count)?;
                pages.extend(start..=end);
            } else {
                let page = parse_page_number(part, part)?;
                validate_page(page, page_count)?;
                pages.push(page);
            }
        }
        Self::from_pages(pages)
    }

    pub(crate) fn pages(&self) -> &[u32] {
        &self.pages
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub(crate) fn contains(&self, page: u32) -> bool {
        self.pages.binary_search(&page).is_ok()
    }

    pub(crate) fn canonical_string(&self) -> String {
        let mut ranges = Vec::new();
        let mut pages = self.pages.iter().copied();
        let Some(first_page) = pages.next() else {
            return String::new();
        };

        let mut range_start = first_page;
        let mut range_end = first_page;
        for page in pages {
            if page == range_end + 1 {
                range_end = page;
                continue;
            }
            ranges.push(format_range(range_start, range_end));
            range_start = page;
            range_end = page;
        }
        ranges.push(format_range(range_start, range_end));
        ranges.join(",")
    }
}

fn parse_page_number(value: &str, source: &str) -> Result<u32, PageSetError> {
    if value.is_empty() {
        return Err(PageSetError::InvalidRange(source.to_string()));
    }
    let page = value
        .parse::<u32>()
        .map_err(|_| PageSetError::InvalidPage(value.to_string()))?;
    if page == 0 {
        return Err(PageSetError::InvalidPage(value.to_string()));
    }
    Ok(page)
}

fn validate_page(page: u32, page_count: u32) -> Result<(), PageSetError> {
    if page > page_count {
        return Err(PageSetError::OutOfBounds { page, page_count });
    }
    Ok(())
}

fn validate_range(range: RangeInclusive<u32>, page_count: u32) -> Result<(), PageSetError> {
    let Some(last_page) = range.last() else {
        return Ok(());
    };
    validate_page(last_page, page_count)
}

fn format_range(start: u32, end: u32) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

#[cfg(test)]
mod tests {
    use super::{PageSet, PageSetError};

    #[test]
    fn parses_and_canonicalizes_ranges() {
        let page_set = PageSet::parse("5, 1-3, 3, 8", 8).expect("valid page set");

        assert_eq!(page_set.pages(), &[1, 2, 3, 5, 8]);
        assert_eq!(page_set.canonical_string(), "1-3,5,8");
    }

    #[test]
    fn rejects_reversed_ranges() {
        let error = PageSet::parse("4-2", 5).expect_err("reversed range must fail");

        assert_eq!(error, PageSetError::InvalidRange("4-2".to_string()));
    }

    #[test]
    fn rejects_zero_and_out_of_bounds_pages() {
        assert!(matches!(
            PageSet::parse("0", 5),
            Err(PageSetError::InvalidPage(_))
        ));
        assert_eq!(
            PageSet::parse("6", 5).expect_err("out of bounds page must fail"),
            PageSetError::OutOfBounds {
                page: 6,
                page_count: 5
            }
        );
    }

    #[test]
    fn empty_page_set_is_stable() {
        let page_set = PageSet::parse("", 0).expect("empty selection is valid");

        assert!(page_set.is_empty());
        assert_eq!(page_set.canonical_string(), "");
        assert!(!page_set.contains(1));
    }

    #[test]
    fn all_pages_requires_positive_page_count() {
        assert!(PageSet::all(0).is_err());
        assert_eq!(
            PageSet::all(4).expect("all pages").canonical_string(),
            "1-4"
        );
    }
}
