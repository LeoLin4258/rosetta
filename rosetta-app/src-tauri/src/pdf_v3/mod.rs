#![cfg_attr(any(feature = "experimental-pdf-v3", test), allow(dead_code))]

#[cfg(all(test, target_os = "windows"))]
mod acceptance;

// Production still consumes selected identity and translation-plan types from
// these modules. Keep dead-code suppression local to those shared primitives.
#[allow(dead_code)]
pub(crate) mod document;
#[allow(dead_code)]
pub(crate) mod object_delta;
#[allow(dead_code)]
pub(crate) mod paragraph_translation_plan;
#[allow(dead_code)]
pub(crate) mod source_object;
#[allow(dead_code)]
pub(crate) mod translation_patch;
#[allow(dead_code)]
pub(crate) mod translation_plan;
#[allow(dead_code)]
pub(crate) mod types;

#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod content_stream;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod extract;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod font;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod font_plan;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod identity;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod incremental_export;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod layout;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod legacy_adapter;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod mapping;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod ownership;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod page_context;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod page_graph_store;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod page_index;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod page_pdf;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod page_set;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod patch;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod patch_renderer;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod patch_store;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod pipeline;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod preview;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod reconcile;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod region_layout;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod region_renderer;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod region_translation_patch;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod render_cache;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod replacement;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod scheduler;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod source_cmap;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod style;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod translation_export;
#[cfg(any(feature = "experimental-pdf-v3", test))]
pub(crate) mod visual_grouping;
