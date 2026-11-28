//! Native context cookies (`Storage.*` per browser context) and main-frame
//! `WebStorage` in the utility world, as the pinned Chromium server does.
use super::http::{cookie_view, cookies_for_write};
use super::page::execution_error;
use super::*;
use crate::browser::{http::CookieFilter, storage};

impl Worker {
    async fn native_context_cookies(
        &mut self,
        id: Uuid,
        context: Reference,
        deadline: Instant,
    ) -> Result<Vec<Value>, Error> {
        let native = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        let reply = self
            .call_read(
                id,
                None,
                "Storage.getCookies",
                json!({"browserContextId":native}),
                deadline,
            )
            .await?;
        cookie_view(reply["cookies"].clone())
    }
    async fn native_set_cookies(
        &mut self,
        id: Uuid,
        context: Reference,
        cookies: Vec<Value>,
        deadline: Instant,
    ) -> Result<(), Error> {
        let native = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        let cookies = cookies_for_write(json!(cookies))?;
        self.call(
            id,
            None,
            "Storage.setCookies",
            json!({"browserContextId":native,"cookies":cookies}),
            deadline,
        )
        .await?;
        Ok(())
    }
    pub(super) async fn native_context_cookies_read(
        &mut self,
        id: Uuid,
        context: Reference,
        urls: crate::browser::http::CookieUrls,
        cursor: Option<String>,
        limit: Option<u64>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        urls.parse()?;
        let cookies = self.native_context_cookies(id, context, deadline).await?;
        storage::cookies::page(context, &json!(cookies), &urls, cursor, limit)
    }
    pub(super) async fn native_context_cookies_add(
        &mut self,
        id: Uuid,
        context: Reference,
        cookies: Vec<Value>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let cookies = storage::cookies::added(cookies)?;
        let count = cookies.len();
        self.native_set_cookies(id, context, cookies, deadline)
            .await?;
        Ok(storage::cookies::add_result(context, count))
    }
    /// Unfiltered clears use `Storage.clearCookies`; a filter expires the
    /// cookies it selects, as the pinned server's `clearCookies` does.
    pub(super) async fn native_context_cookies_clear(
        &mut self,
        id: Uuid,
        context: Reference,
        options: CookieFilter,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        if options.is_empty() {
            let native = self
                .index
                .native(context, Kind::Context)
                .map_err(resource_error)?;
            self.call(
                id,
                None,
                "Storage.clearCookies",
                json!({"browserContextId":native}),
                deadline,
            )
            .await?;
            return Ok(storage::cookies::clear_result(context, None));
        }
        let values = self.native_context_cookies(id, context, deadline).await?;
        let selected = self
            .await_http_preparation(
                id,
                storage::cookies::expired(values, options, deadline),
                deadline,
            )
            .await?;
        let count = selected.len();
        if !selected.is_empty() {
            // `native_set_cookies` revalidates the context after matching.
            self.native_set_cookies(id, context, selected, deadline)
                .await?;
        }
        Ok(storage::cookies::clear_result(context, Some(count)))
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "one WebStorage read and its item or page selectors"
    )]
    pub(super) async fn native_page_web_storage_read(
        &mut self,
        id: Uuid,
        page: Reference,
        kind: storage::web::Kind,
        name: Option<String>,
        cursor: Option<String>,
        limit: Option<u64>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        storage::web::validate_read(name.as_deref(), cursor.as_deref(), limit)?;
        let scope = self.page_storage_scope(id, page, None, deadline).await?;
        if let Some(name) = name {
            let value = self
                .page_expression(
                    id,
                    &scope,
                    storage::web::get_expression(kind, &name),
                    deadline,
                )
                .await?;
            return storage::web::item(page, kind, &name, &value);
        }
        let items = self
            .page_expression(id, &scope, storage::web::items_expression(kind), deadline)
            .await?;
        storage::web::page(page, kind, &items, cursor, limit)
    }
    /// Cache Storage resolves an origin within a target's partition, so it
    /// uses a live page session of the context.
    fn context_page_session(&self, context: Reference) -> Result<String, Error> {
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        let page = self
            .index
            .list(Kind::Page)
            .into_iter()
            .find(|(_, parent)| *parent == Some(context))
            .map(|(page, _)| page)
            .ok_or("CACHE_STORAGE_PAGE_REQUIRED")?;
        let frame = self.index.page_frame(page, None).map_err(resource_error)?;
        Ok(self
            .index
            .route(frame, Some(Kind::Frame))
            .map_err(resource_error)?
            .session_id)
    }
    async fn native_cache_entries(
        &mut self,
        id: Uuid,
        session: &str,
        cache: &str,
        skip: u64,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.call_read(
            id,
            Some(session),
            "CacheStorage.requestEntries",
            json!({"cacheId":cache,"skipCount":skip,"pageSize":storage::cache::WINDOW}),
            deadline,
        )
        .await
    }
    /// A URL read scans the cache window by window, so it reaches entries
    /// past the bound a listing refuses.
    async fn native_cache_entry(
        &mut self,
        id: Uuid,
        session: &str,
        cache: &str,
        url: &str,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let mut skip = 0;
        loop {
            let entries = self
                .native_cache_entries(id, session, cache, skip, deadline)
                .await?;
            match storage::cache::lookup(&entries, url, skip)? {
                storage::cache::Lookup::Found(found) => return Ok(found),
                storage::cache::Lookup::Next(next) => skip = next,
            }
        }
    }
    async fn native_caches(
        &mut self,
        id: Uuid,
        session: &str,
        origin: &str,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.call_read(
            id,
            Some(session),
            "CacheStorage.requestCacheNames",
            json!({"securityOrigin":origin}),
            deadline,
        )
        .await
    }
    async fn native_cache_id(
        &mut self,
        id: Uuid,
        session: &str,
        origin: &str,
        name: &str,
        deadline: Instant,
    ) -> Result<String, Error> {
        let caches = self.native_caches(id, session, origin, deadline).await?;
        storage::cache::cache_id(&caches, name)
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "one paged Cache Storage read and its selectors"
    )]
    pub(super) async fn native_cache_storage_read(
        &mut self,
        id: Uuid,
        context: Reference,
        origin: String,
        cache_name: Option<String>,
        url: Option<String>,
        cursor: Option<String>,
        limit: Option<u64>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        storage::cache::validate_read(
            &origin,
            url.as_deref(),
            cache_name.as_deref(),
            cursor.as_deref(),
            limit,
        )?;
        let session = self.context_page_session(context)?;
        let Some(name) = cache_name else {
            let caches = self.native_caches(id, &session, &origin, deadline).await?;
            return storage::cache::caches_page(context, &origin, &caches, cursor, limit);
        };
        let cache = self
            .native_cache_id(id, &session, &origin, &name, deadline)
            .await?;
        let Some(url) = url else {
            let entries = self
                .native_cache_entries(id, &session, &cache, 0, deadline)
                .await?;
            return storage::cache::entries_page(context, &origin, &name, &entries, cursor, limit);
        };
        let found = self
            .native_cache_entry(id, &session, &cache, &url, deadline)
            .await?;
        let body = self
            .call_read(
                id,
                Some(&session),
                "CacheStorage.requestCachedResponse",
                json!({"cacheId":cache,"requestURL":url,"requestHeaders":found["requestHeaders"]}),
                deadline,
            )
            .await?;
        storage::cache::response(context, &origin, &name, &found, &body)
    }
    pub(super) async fn native_cache_storage_delete(
        &mut self,
        id: Uuid,
        context: Reference,
        origin: String,
        cache_name: String,
        url: Option<String>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        storage::cache::validate_origin(&origin)?;
        let session = self.context_page_session(context)?;
        let cache = self
            .native_cache_id(id, &session, &origin, &cache_name, deadline)
            .await?;
        match &url {
            Some(url) => {
                self.call(
                    id,
                    Some(&session),
                    "CacheStorage.deleteEntry",
                    json!({"cacheId":cache,"request":url}),
                    deadline,
                )
                .await?
            }
            None => {
                self.call(
                    id,
                    Some(&session),
                    "CacheStorage.deleteCache",
                    json!({"cacheId":cache}),
                    deadline,
                )
                .await?
            }
        };
        Ok(storage::cache::deleted(
            context,
            &origin,
            &cache_name,
            url.as_deref(),
        ))
    }
    /// Writes dispatch through `call`, so a cancelled or lost write settles as
    /// an unknown outcome instead of a safe refusal.
    pub(super) async fn native_page_web_storage_update(
        &mut self,
        id: Uuid,
        page: Reference,
        kind: storage::web::Kind,
        update: storage::web::Update,
        deadline: Instant,
    ) -> Result<Value, Error> {
        update.validate()?;
        let scope = self.page_storage_scope(id, page, None, deadline).await?;
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        let result = self
            .call(
                id,
                Some(&scope.session),
                "Runtime.evaluate",
                json!({"expression":update.expression(kind),"uniqueContextId":scope.unique,"returnByValue":true,"awaitPromise":true}),
                deadline,
            )
            .await?;
        if result.get("exceptionDetails").is_some() {
            return Err(execution_error(&result));
        }
        Ok(update.result(page, kind))
    }
}
