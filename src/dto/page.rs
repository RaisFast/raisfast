use serde::{Deserialize, Serialize};
#[cfg(feature = "export-types")]
use ts_rs::TS;
use utoipa::ToSchema;
use validator::Validate;

use crate::models::page::{Page, PageStatus};

#[cfg_attr(feature = "export-types", derive(TS))]
#[derive(Debug, Serialize, ToSchema)]
pub struct PageResponse {
    pub id: String,
    pub title: String,
    pub slug: String,
    pub content: Option<String>,
    pub blocks: Option<String>,
    pub status: String,
    pub template: String,
    #[cfg_attr(feature = "export-types", ts(type = "number"))]
    pub sort_order: i64,
    pub cover_image: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl PageResponse {
    pub fn from_page(p: Page) -> Self {
        Self {
            id: p.id.to_string(),
            title: p.title,
            slug: p.slug,
            content: p.content,
            blocks: p.blocks,
            status: p.status.to_string(),
            template: p.template,
            sort_order: p.sort_order,
            cover_image: p.cover_image,
            created_at: p.created_at.to_rfc3339(),
            updated_at: p.updated_at.to_rfc3339(),
        }
    }
}

#[cfg_attr(feature = "export-types", derive(TS))]
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreatePageRequest {
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    pub slug: Option<String>,
    pub content: Option<String>,
    pub blocks: Option<String>,
    pub meta_title: Option<String>,
    pub meta_description: Option<String>,
    pub og_image: Option<String>,
    pub template: Option<String>,
    pub parent_id: Option<String>,
    pub sort_order: Option<i64>,
    pub status: Option<PageStatus>,
    pub cover_image: Option<String>,
}

#[cfg_attr(feature = "export-types", derive(TS))]
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdatePageRequest {
    #[validate(length(min = 1, max = 200))]
    pub title: Option<String>,
    pub slug: Option<String>,
    pub content: Option<String>,
    pub blocks: Option<String>,
    pub meta_title: Option<String>,
    pub meta_description: Option<String>,
    pub og_image: Option<String>,
    pub template: Option<String>,
    pub parent_id: Option<Option<String>>,
    pub sort_order: Option<i64>,
    pub status: Option<PageStatus>,
    pub cover_image: Option<String>,
}

#[cfg_attr(feature = "export-types", derive(TS))]
#[derive(Debug, Deserialize)]
pub struct PageListQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[cfg_attr(feature = "export-types", derive(TS))]
#[derive(Debug, Deserialize)]
pub struct AdminPageListQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub status: Option<PageStatus>,
}

#[cfg_attr(feature = "export-types", derive(TS))]
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateStatusRequest {
    pub status: PageStatus,
}

#[cfg_attr(feature = "export-types", derive(TS))]
#[derive(Debug, Deserialize, ToSchema)]
pub struct ReorderItem {
    pub id: String,
    pub sort_order: i64,
}

#[cfg_attr(feature = "export-types", derive(TS))]
#[derive(Debug, Deserialize, ToSchema)]
pub struct ReorderRequest {
    pub items: Vec<ReorderItem>,
}

#[cfg_attr(feature = "export-types", derive(TS))]
#[derive(Debug, Serialize)]
pub struct SitemapEntry {
    pub slug: String,
    pub updated_at: Option<String>,
}
