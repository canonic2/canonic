//! Resolves relative request URLs against the owning context's `baseURL`, like
//! the pinned Playwright client, before each adapter's strict URL validation.
use super::{
    contract::{Reference, Request},
    page::Navigation,
    routing::HandlerAction,
};

/// Placeholder base used only to check relative syntax before a context is known.
const PROBE: &str = "http://testrun.invalid/";

impl Request {
    /// Whether this request carries a URL that a context `baseURL` can resolve.
    pub(crate) fn uses_base(&self) -> bool {
        match self {
            Self::Batch { requests } => requests.iter().any(Self::uses_base),
            Self::NetworkRoute { action, .. } => matches!(
                action,
                HandlerAction::Add { .. }
                    | HandlerAction::AddMatcher { .. }
                    | HandlerAction::RemoveMatcher { .. }
                    | HandlerAction::AddHar {
                        url_filter: Some(_),
                        ..
                    }
            ),
            Self::PageRouteSet { url, .. } | Self::ContextRouteSet { url, .. } => url.is_some(),
            Self::PageHarSet { url_filter, .. }
            | Self::ContextHarSet { url_filter, .. }
            | Self::HarCaptureStart { url_filter, .. } => url_filter.is_some(),
            Self::PageNavigate { navigation, .. } => matches!(navigation, Navigation::Goto { .. }),
            Self::WebSocketRouteAdd { .. } | Self::WebSocketRouteAddMatcher { .. } => true,
            Self::DiagnosticsWait { conditions, .. } => {
                conditions.iter().any(|condition| condition.url.is_some())
            }
            Self::PageWait { condition, .. } => matches!(
                condition,
                super::page::Condition::Url { .. } | super::page::Condition::UrlMatch { .. }
            ),
            _ => false,
        }
    }
    /// Resolve every relative URL in place; `base` returns the scope's context base.
    /// A scope without a base leaves its URLs unchanged for strict validation.
    pub(crate) fn resolve_base(&mut self, base: &mut impl FnMut(Reference) -> Option<url::Url>) {
        let scope = match self {
            Self::Batch { requests } => {
                for request in requests {
                    request.resolve_base(base);
                }
                return;
            }
            Self::NetworkRoute { scope, .. }
            | Self::WebSocketRouteAdd { scope, .. }
            | Self::WebSocketRouteAddMatcher { scope, .. } => *scope,
            Self::PageRouteSet { page, .. }
            | Self::PageHarSet { page, .. }
            | Self::PageNavigate { page, .. }
            | Self::PageWait { page, .. } => *page,
            Self::ContextRouteSet { context, .. }
            | Self::ContextHarSet { context, .. }
            | Self::HarCaptureStart { context, .. }
            | Self::DiagnosticsWait { context, .. } => *context,
            _ => return,
        };
        if !self.uses_base() {
            return;
        }
        let Some(base) = base(scope) else {
            return;
        };
        let join = super::routing::join_base;
        match self {
            Self::NetworkRoute { action, .. } => match action {
                HandlerAction::Add { url, .. } => *url = join(&base, url),
                HandlerAction::AddMatcher { matcher, .. }
                | HandlerAction::RemoveMatcher { matcher }
                | HandlerAction::AddHar {
                    url_filter: Some(matcher),
                    ..
                } => matcher.resolve(&base),
                _ => {}
            },
            Self::PageRouteSet { url: Some(url), .. }
            | Self::ContextRouteSet { url: Some(url), .. } => *url = join(&base, url),
            Self::PageHarSet {
                url_filter: Some(matcher),
                ..
            }
            | Self::ContextHarSet {
                url_filter: Some(matcher),
                ..
            }
            | Self::HarCaptureStart {
                url_filter: Some(matcher),
                ..
            } => matcher.resolve(&base),
            Self::PageNavigate {
                navigation: Navigation::Goto { url, .. },
                ..
            } => *url = join(&base, url),
            Self::WebSocketRouteAdd { url, .. } => {
                *url = join(&super::socket_routes::websocket_base(&base), url);
            }
            Self::WebSocketRouteAddMatcher { matcher, .. } => matcher.resolve(&base),
            Self::PageWait { condition, .. } => match condition {
                super::page::Condition::Url { value } => *value = join(&base, value),
                super::page::Condition::UrlMatch { matcher, .. } => matcher.resolve(&base),
                _ => {}
            },
            Self::DiagnosticsWait { conditions, .. } => {
                let socket = super::socket_routes::websocket_base(&base);
                for condition in conditions {
                    if let Some(url) = &mut condition.url {
                        let base =
                            if condition.event == super::diagnostics::DiagnosticKind::WebSocket {
                                &socket
                            } else {
                                &base
                            };
                        *url = join(base, url);
                    }
                }
            }
            _ => {}
        }
    }
    /// A copy resolved against a placeholder base, for submission-time syntax checks.
    /// Dispatch resolves the original against the real base and validates it again.
    pub(crate) fn syntax_probe(&self) -> Option<Self> {
        self.uses_base().then(|| {
            let mut probe = self.clone();
            let base = url::Url::parse(PROBE).expect("Static probe base");
            probe.resolve_base(&mut |_| Some(base.clone()));
            probe
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::routing::Matcher;
    use serde_json::json;
    use uuid::Uuid;

    fn reference() -> Reference {
        Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    fn base() -> url::Url {
        url::Url::parse("https://Example.com/acme/app/").unwrap()
    }
    fn matcher(value: serde_json::Value) -> Matcher {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn relative_matchers_follow_the_pinned_glob_base_resolution() {
        for (input, expected) in [
            (
                json!({"kind":"glob","pattern":"/api/*"}),
                json!({"kind":"glob","pattern":"https://example.com/api/*"}),
            ),
            (
                json!({"kind":"glob","pattern":"data/**?item=*"}),
                json!({"kind":"glob","pattern":"https://example.com/acme/app/data/**?item=*"}),
            ),
            (
                json!({"kind":"glob","pattern":"../{one,two}.json"}),
                json!({"kind":"glob","pattern":"https://example.com/acme/{one,two}.json"}),
            ),
            (
                json!({"kind":"glob","pattern":"**/api/*"}),
                json!({"kind":"glob","pattern":"**/api/*"}),
            ),
            (
                json!({"kind":"exact","url":"item?id=1"}),
                json!({"kind":"exact","url":"https://example.com/acme/app/item?id=1"}),
            ),
            (
                json!({"kind":"regex","pattern":"/api/","flags":"i"}),
                json!({"kind":"regex","pattern":"/api/","flags":"i"}),
            ),
        ] {
            let mut resolved = matcher(input);
            resolved.resolve(&base());
            assert_eq!(resolved, matcher(expected));
            resolved.compile().unwrap();
        }
        let mut absolute = matcher(json!({"kind":"glob","pattern":"https://other.example/*"}));
        absolute.resolve(&base());
        assert_eq!(
            absolute,
            matcher(json!({"kind":"glob","pattern":"https://other.example/*"}))
        );
    }

    #[test]
    fn context_base_is_validated_testrun_owned_and_accepted_on_every_engine() {
        use crate::browser::contract::{ContextOptions, Engine};
        let options = |base: &str| ContextOptions {
            base_url: Some(base.into()),
            ..ContextOptions::default()
        };
        for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
            options("https://example.com/acme/")
                .validate_for(engine)
                .unwrap();
        }
        assert!(
            options("https://example.com/acme/")
                .wire()
                .get("baseURL")
                .is_none()
        );
        for invalid in [
            "/relative",
            "file:///tmp/acme",
            "https://user:secret@example.com/",
            "https://example.com/#fragment",
            "https://example.com/\u{7f}",
        ] {
            assert_eq!(
                options(invalid).validate(),
                Err("INVALID_BASE_URL"),
                "{invalid}"
            );
        }
    }
    #[test]
    fn record_har_defaults_to_full_resolves_its_filter_and_refuses_invalid_syntax() {
        use crate::browser::contract::{ContextOptions, Engine, HarCaptureContent, HarCaptureMode};
        let options: ContextOptions = serde_json::from_value(json!({"viewport":{"width":800,"height":600},"deviceScaleFactor":1,"baseURL":"https://example.com/acme/","recordHar":{"urlFilter":{"kind":"glob","pattern":"api/*"}}})).unwrap();
        let record = options.record_har.as_ref().unwrap();
        assert_eq!(record.mode, HarCaptureMode::Full);
        assert_eq!(record.options().unwrap().0, HarCaptureContent::Embed);
        assert_eq!(
            record.filter(&options),
            Some(matcher(
                json!({"kind":"glob","pattern":"https://example.com/acme/api/*"})
            ))
        );
        for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
            options.validate_for(engine).unwrap();
        }
        assert!(options.wire().get("recordHar").is_none());
        let invalid: ContextOptions = serde_json::from_value(json!({"viewport":{"width":800,"height":600},"deviceScaleFactor":1,"recordHar":{"urlFilter":{"kind":"regex","pattern":"("}}})).unwrap();
        assert!(invalid.validate().is_err());
        // Pinned `content ?? (omitContent ? "omit" : default)`.
        for (record, expected) in [
            (json!({"omitContent":true}), HarCaptureContent::Omit),
            (
                json!({"omitContent":true,"content":"embed"}),
                HarCaptureContent::Embed,
            ),
            (
                json!({"omitContent":false,"content":"attach"}),
                HarCaptureContent::Attach,
            ),
        ] {
            let options: ContextOptions = serde_json::from_value(json!({"viewport":{"width":800,"height":600},"deviceScaleFactor":1,"recordHar":record})).unwrap();
            assert_eq!(options.record_har.unwrap().options().unwrap().0, expected);
        }
        let shared: ContextOptions = serde_json::from_value(json!({"viewport":{"width":800,"height":600},"deviceScaleFactor":1,"recordHar":{"path":"/tmp/acme.har"}})).unwrap();
        assert!(shared.record_har.unwrap().options().is_err());
    }
    #[test]
    fn page_url_waits_resolve_and_match_with_one_compiled_matcher_per_request() {
        let page = reference();
        let wait = |condition: serde_json::Value| -> Request {
            serde_json::from_value(
                json!({"operation":"page_wait","page":page,"condition":condition}),
            )
            .unwrap()
        };
        let mut exact = wait(json!({"kind":"url","value":"done?step=2"}));
        let mut glob =
            wait(json!({"kind":"url_match","matcher":{"kind":"glob","pattern":"**/done?step=*"}}));
        let probe = glob.syntax_probe().unwrap();
        for request in [&mut exact, &mut glob] {
            assert!(request.uses_base());
            request.resolve_base(&mut |_| Some(base()));
        }
        let Request::PageWait { condition, .. } = &exact else {
            unreachable!()
        };
        assert!(
            matches!(condition, crate::browser::page::Condition::Url { value } if value == "https://example.com/acme/app/done?step=2")
        );
        let Request::PageWait { condition, .. } = &glob else {
            unreachable!()
        };
        condition.validate().unwrap();
        let done = "https://example.com/acme/app/done?step=2";
        assert_eq!(
            condition.settle(json!({"href":done,"loadState":"load"})),
            json!({"satisfied":true})
        );
        // Pinned default `waitUntil: "load"`: a matching URL waits for the load event.
        for state in ["commit", "domcontentloaded"] {
            assert_eq!(
                condition.settle(json!({"href":done,"loadState":state})),
                json!({"satisfied":false})
            );
        }
        for (wait_until, state, satisfied) in [
            ("commit", "commit", true),
            ("domcontentloaded", "commit", false),
            ("domcontentloaded", "domcontentloaded", true),
            ("domcontentloaded", "load", true),
        ] {
            let Request::PageWait { condition, .. } = wait(
                json!({"kind":"url_match","matcher":{"kind":"exact","url":done},"waitUntil":wait_until}),
            ) else {
                unreachable!()
            };
            assert_eq!(
                condition.settle(json!({"href":done,"loadState":state})),
                json!({"satisfied":satisfied}),
                "{wait_until}/{state}"
            );
        }
        let Request::PageWait { condition, .. } = wait(
            json!({"kind":"url_match","matcher":{"kind":"exact","url":done},"waitUntil":"networkidle"}),
        ) else {
            unreachable!()
        };
        condition.validate().unwrap();
        assert!(condition.needs_network_idle());
        for (state, idle, satisfied) in [
            ("load", true, true),
            ("load", false, false),
            ("domcontentloaded", true, false),
        ] {
            assert_eq!(
                condition.settle(json!({"href":done,"loadState":state,"networkIdle":idle})),
                json!({"satisfied":satisfied}),
                "{state}/{idle}"
            );
        }
        assert_eq!(
            condition.settle(json!({"href":"https://example.com/acme/app/other"})),
            json!({"satisfied":false})
        );
        assert_eq!(condition.settle(json!({})), json!({"satisfied":false}));
        let relative =
            wait(json!({"kind":"url_match","matcher":{"kind":"glob","pattern":"done/*"}}));
        let Request::PageWait { condition, .. } = &relative else {
            unreachable!()
        };
        assert!(
            condition.validate().is_err(),
            "an unbased relative glob stays invalid"
        );
        let Request::PageWait { condition, .. } = &probe else {
            unreachable!()
        };
        condition.validate().unwrap();
        assert!(
            serde_json::to_value(&glob)
                .unwrap()
                .to_string()
                .find("compiled")
                .is_none()
        );
        let visible = wait(json!({"kind":"visible"}));
        assert!(!visible.uses_base());
        let Request::PageWait { condition, .. } = &visible else {
            unreachable!()
        };
        assert_eq!(
            condition.settle(json!({"satisfied":true})),
            json!({"satisfied":true})
        );
    }
    #[test]
    fn diagnostic_wait_urls_resolve_with_http_or_socket_bases() {
        let context = reference();
        let mut request: Request = serde_json::from_value(
            json!({"operation":"diagnostics_wait","context":context,"after":0,"conditions":[
                {"event":"response","url":"data/item","status":200},
                {"event":"web_socket","url":"/socket"},
                {"event":"request"}
            ]}),
        )
        .unwrap();
        assert!(request.uses_base());
        request.resolve_base(&mut |scope| {
            assert_eq!(scope, context);
            Some(base())
        });
        let Request::DiagnosticsWait { conditions, .. } = &request else {
            unreachable!()
        };
        assert_eq!(
            conditions[0].url.as_deref(),
            Some("https://example.com/acme/app/data/item")
        );
        assert_eq!(
            conditions[1].url.as_deref(),
            Some("wss://example.com/socket")
        );
        assert!(conditions[2].url.is_none());
        let unconditioned: Request = serde_json::from_value(json!({"operation":"diagnostics_wait","context":context,"after":0,"conditions":[{"event":"request"}]})).unwrap();
        assert!(!unconditioned.uses_base());
    }
    #[test]
    fn browser_associated_http_inherits_only_an_unset_base() {
        use crate::browser::{contract::ContextOptions, http::Options};
        let context = ContextOptions {
            base_url: Some("https://example.com/acme/".into()),
            ..ContextOptions::default()
        };
        let mut inherited = Options::default();
        inherited.inherit(&context);
        assert_eq!(
            inherited.base_url.as_deref(),
            Some("https://example.com/acme/")
        );
        let mut explicit = Options {
            base_url: Some("https://example.com/other/".into()),
            ..Options::default()
        };
        explicit.inherit(&context);
        assert_eq!(
            explicit.base_url.as_deref(),
            Some("https://example.com/other/")
        );
    }
    #[test]
    fn route_from_har_filters_resolve_against_the_context_base() {
        let page = reference();
        let archive = uuid::Uuid::new_v4();
        let mut request: Request = serde_json::from_value(json!({"operation":"network_route","scope":page,"action":{"kind":"add_har","archiveId":archive,"urlFilter":{"kind":"glob","pattern":"api/*"}}})).unwrap();
        assert!(request.uses_base());
        request.resolve_base(&mut |_| Some(base()));
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(
            value["action"]["urlFilter"]["pattern"],
            "https://example.com/acme/app/api/*"
        );
        let unfiltered: Request = serde_json::from_value(json!({"operation":"network_route","scope":page,"action":{"kind":"add_har","archiveId":archive}})).unwrap();
        assert!(!unfiltered.uses_base());
        use crate::browser::har::CaptureContent;
        assert_eq!(
            CaptureContent::for_capture(None, Some(archive)),
            CaptureContent::Attach
        );
        assert_eq!(
            CaptureContent::for_capture(None, None),
            CaptureContent::Omit
        );
        assert_eq!(
            CaptureContent::for_capture(Some(CaptureContent::Embed), Some(archive)),
            CaptureContent::Embed
        );
    }
    #[test]
    fn request_resolution_uses_the_scope_base_and_leaves_unbased_urls_for_refusal() {
        let page = reference();
        let mut request: Request = serde_json::from_value(json!({"operation":"batch","requests":[
            {"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":"next?step=2"}},
            {"operation":"web_socket_route_add","scope":page,"url":"/socket"},
            {"operation":"har_capture_start","context":page,"urlFilter":{"kind":"glob","pattern":"/api/**"}}
        ]}))
        .unwrap();
        assert!(request.uses_base());
        let probe = request.syntax_probe().unwrap();
        let mut unbased = request.clone();
        unbased.resolve_base(&mut |_| None);
        assert_eq!(
            serde_json::to_value(&unbased).unwrap(),
            serde_json::to_value(&request).unwrap()
        );
        let mut seen = Vec::new();
        request.resolve_base(&mut |scope| {
            seen.push(scope);
            Some(base())
        });
        assert_eq!(seen, vec![page; 3]);
        let value = serde_json::to_value(&request).unwrap();
        let text = value.to_string();
        assert!(
            text.contains("https://example.com/acme/app/next?step=2"),
            "{text}"
        );
        assert!(text.contains("wss://example.com/socket"), "{text}");
        assert!(text.contains("https://example.com/api/**"), "{text}");
        assert!(
            serde_json::to_value(&probe)
                .unwrap()
                .to_string()
                .contains(PROBE)
        );
        assert!(
            serde_json::from_value::<Request>(json!({"operation":"har_list"}))
                .unwrap()
                .syntax_probe()
                .is_none()
        );
    }
}
