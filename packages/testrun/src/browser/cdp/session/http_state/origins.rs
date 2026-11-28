//! Context lifetime, insertion-ordered origins and attachment-ordered live pages.
use crate::browser::contract::Reference;
use std::collections::BTreeMap;

pub(super) const MAX_ORIGINS: usize = 1024;
const MAX_ORIGIN_BYTES: usize = 2048;
#[derive(Default)]
struct History {
    origins: BTreeMap<String, usize>,
    pages: BTreeMap<Reference, (usize, Option<String>)>,
    next_page: usize,
    failure: Option<&'static str>,
}
#[derive(Default)]
pub(super) struct Origins(BTreeMap<Reference, History>);
pub(super) struct Plan {
    pub(super) origins: Vec<String>,
    pub(super) pages: Vec<(Reference, String)>,
}
pub(super) fn origin(url: &str) -> Result<Option<String>, &'static str> {
    if !url.starts_with("http") {
        return Ok(None);
    }
    let Ok(parsed) = url::Url::parse(url) else {
        return Ok(None);
    };
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Ok(None);
    }
    let origin = parsed.origin().ascii_serialization();
    if origin.len() > MAX_ORIGIN_BYTES {
        return Err("HTTP_STORAGE_ORIGIN_BOUNDS");
    }
    Ok(Some(origin))
}
impl Origins {
    pub(super) fn attach(&mut self, context: Reference, page: Reference) {
        let history = self.0.entry(context).or_default();
        if history.failure.is_none() && !history.pages.contains_key(&page) {
            let order = history.next_page;
            let Some(next) = order.checked_add(1) else {
                history.failure = Some("HTTP_STORAGE_ORIGIN_BOUNDS");
                return;
            };
            history.next_page = next;
            history.pages.insert(page, (order, None));
        }
    }
    pub(super) fn navigate(
        &mut self,
        context: Reference,
        page: Reference,
        origin: Result<Option<String>, &'static str>,
        main: bool,
    ) {
        self.attach(context, page);
        let history = self.0.get_mut(&context).expect("Attached context");
        if history.failure.is_some() {
            return;
        }
        let origin = match origin {
            Ok(origin) => origin,
            Err(code) => {
                history.failure = Some(code);
                return;
            }
        };
        if let Some(origin) = &origin
            && !history.origins.contains_key(origin)
        {
            if history.origins.len() >= MAX_ORIGINS {
                history.failure = Some("HTTP_STORAGE_ORIGIN_BOUNDS");
                return;
            }
            let order = history.origins.len();
            history.origins.insert(origin.clone(), order);
        }
        if main {
            history.pages.get_mut(&page).expect("Attached page").1 = origin;
        }
    }
    pub(super) fn plan(&self, context: Reference) -> Result<Plan, &'static str> {
        let Some(history) = self.0.get(&context) else {
            return Ok(Plan {
                origins: vec![],
                pages: vec![],
            });
        };
        if let Some(failure) = history.failure {
            return Err(failure);
        }
        let mut origins = history.origins.iter().collect::<Vec<_>>();
        origins.sort_by_key(|(_, order)| **order);
        let mut pages = history.pages.iter().collect::<Vec<_>>();
        pages.sort_by_key(|(_, (order, _))| *order);
        Ok(Plan {
            origins: origins
                .into_iter()
                .map(|(origin, _)| origin.clone())
                .collect(),
            pages: pages
                .into_iter()
                .filter_map(|(page, (_, origin))| {
                    origin.as_ref().map(|origin| (*page, origin.clone()))
                })
                .collect(),
        })
    }
    /// After a replacing restore the context has visited exactly the
    /// snapshot's origins, as the pinned `setStorageState` records.
    pub(super) fn replace(&mut self, context: Reference, origins: &[String]) {
        let history = self.0.entry(context).or_default();
        if history.failure.is_some() {
            return;
        }
        history.origins = origins
            .iter()
            .take(MAX_ORIGINS)
            .enumerate()
            .map(|(order, origin)| (origin.clone(), order))
            .collect();
    }
    /// Remember origins a merge wrote, after those already visited.
    pub(super) fn extend(&mut self, context: Reference, origins: &[String]) {
        let history = self.0.entry(context).or_default();
        for origin in origins {
            if history.failure.is_some() || history.origins.contains_key(origin) {
                continue;
            }
            if history.origins.len() >= MAX_ORIGINS {
                history.failure = Some("HTTP_STORAGE_ORIGIN_BOUNDS");
                return;
            }
            let order = history.origins.len();
            history.origins.insert(origin.clone(), order);
        }
    }
    pub(super) fn close_page(&mut self, page: Reference) {
        for history in self.0.values_mut() {
            history.pages.remove(&page);
        }
    }
    pub(super) fn close_context(&mut self, context: Reference) {
        self.0.remove(&context);
    }
    pub(super) fn gap(&mut self, contexts: impl IntoIterator<Item = Reference>) {
        for context in contexts {
            let history = self.0.entry(context).or_default();
            history.origins.clear();
            history.pages.clear();
            history.failure = Some("HTTP_STORAGE_ORIGIN_HISTORY_LOST");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference(id: u128) -> Reference {
        Reference {
            connection_id: uuid::Uuid::from_u128(1),
            id: uuid::Uuid::from_u128(id),
            generation: uuid::Uuid::from_u128(2),
        }
    }
    #[test]
    fn native_http_state_origin_plan_preserves_context_and_page_order_after_page_closure() {
        let (context, other, first, second, third) = (
            reference(10),
            reference(11),
            reference(20),
            reference(21),
            reference(22),
        );
        let mut origins = Origins::default();
        origins.attach(context, first);
        origins.attach(context, second);
        origins.navigate(context, second, origin("https://b.example.com/path"), true);
        origins.navigate(context, first, origin("https://a.example.com/path"), true);
        origins.navigate(other, third, origin("https://private.example.com"), true);
        let plan = origins.plan(context).unwrap();
        assert_eq!(
            plan.origins,
            ["https://b.example.com", "https://a.example.com"]
        );
        assert_eq!(
            plan.pages,
            [
                (first, "https://a.example.com".into()),
                (second, "https://b.example.com".into())
            ]
        );
        origins.close_page(first);
        let plan = origins.plan(context).unwrap();
        assert_eq!(
            plan.origins,
            ["https://b.example.com", "https://a.example.com"]
        );
        assert_eq!(plan.pages, [(second, "https://b.example.com".into())]);
        origins.close_context(context);
        assert!(origins.plan(context).unwrap().origins.is_empty());
        assert_eq!(
            origins.plan(other).unwrap().origins,
            ["https://private.example.com"]
        );
    }
    #[test]
    fn native_http_state_origin_records_child_frames_without_replacing_the_main_origin() {
        let (context, page) = (reference(10), reference(20));
        let mut origins = Origins::default();
        origins.navigate(
            context,
            page,
            origin("https://acme:private@example.com:443/path?token=private#fragment"),
            true,
        );
        origins.navigate(context, page, origin("https://frame.example.com"), false);
        origins.navigate(context, page, origin("data:text/html,private"), false);
        let plan = origins.plan(context).unwrap();
        assert_eq!(
            plan.origins,
            ["https://example.com", "https://frame.example.com"]
        );
        assert_eq!(plan.pages, [(page, "https://example.com".into())]);
        origins.navigate(context, page, origin("about:blank"), true);
        assert!(origins.plan(context).unwrap().pages.is_empty());
        assert_eq!(origins.plan(context).unwrap().origins.len(), 2);
    }
    #[test]
    fn native_http_state_origin_bounds_and_loss_are_sticky_without_poisoning_new_contexts() {
        let (context, other, page) = (reference(10), reference(11), reference(20));
        let mut origins = Origins::default();
        for index in 0..MAX_ORIGINS {
            origins.navigate(
                context,
                page,
                origin(&format!("https://acme-{index}.example.com")),
                true,
            );
        }
        origins.navigate(context, page, origin("https://acme-0.example.com"), true);
        assert_eq!(origins.plan(context).unwrap().origins.len(), MAX_ORIGINS);
        origins.navigate(context, page, origin("https://overflow.example.com"), true);
        assert!(matches!(
            origins.plan(context),
            Err("HTTP_STORAGE_ORIGIN_BOUNDS")
        ));
        origins.gap([other]);
        origins.navigate(other, page, origin("https://example.com"), true);
        assert!(matches!(
            origins.plan(other),
            Err("HTTP_STORAGE_ORIGIN_HISTORY_LOST")
        ));
        origins.close_context(other);
        let fresh = reference(12);
        origins.navigate(fresh, page, origin("https://example.com"), true);
        assert_eq!(
            origins.plan(fresh).unwrap().origins,
            ["https://example.com"]
        );
    }
}
