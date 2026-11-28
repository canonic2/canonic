//! Immediate selector evaluation; scope and argument authority remain host-owned.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectorOptions {
    pub locator: crate::browser::page::Locator,
    pub expression: String,
    #[serde(default)]
    pub is_function: Option<bool>,
    #[serde(default)]
    pub argument: EvaluationValue,
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub strict: Option<bool>,
    #[serde(default)]
    pub return_handle: bool,
    #[serde(default)]
    pub wait_for_attachment: bool,
    #[serde(default)]
    pub wait_timeout_ms: Option<u64>,
}
impl SelectorOptions {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        self.locator.selector()?;
        if has_reference(&self.locator) {
            return Err("SELECTOR_REFERENCE_UNAVAILABLE");
        }
        if self.expression.is_empty() || self.expression.len() > 8192 {
            return Err("EXPRESSION_BOUNDS");
        }
        if (self.all && (self.strict() || self.wait_for_attachment))
            || (self.wait_for_attachment && self.strict == Some(false))
            || self
                .wait_timeout_ms
                .is_some_and(|ms| !(1..=300000).contains(&ms))
            || (self.wait_timeout_ms.is_some() && !self.wait_for_attachment)
        {
            return Err("SELECTOR_EVALUATION_OPTIONS");
        }
        validate(&self.argument)
    }
    pub(crate) fn native_expression(
        &self,
        frame: Reference,
        relative: bool,
    ) -> Result<String, &'static str> {
        self.expression(
            frame,
            relative,
            "globalThis[Symbol.for('canonic.testrun.selector.v3')]",
            self.wait_timeout_ms.unwrap_or(30000),
        )
    }
    pub(crate) fn strict(&self) -> bool {
        self.strict.unwrap_or(self.wait_for_attachment)
    }
    pub(crate) fn expression(
        &self,
        frame: Reference,
        relative: bool,
        injected: &str,
        timeout_ms: u64,
    ) -> Result<String, &'static str> {
        let options = json!({"selector":self.locator.scoped_selector(frame)?,"expression":self.expression,"isFunction":self.is_function,"all":self.all,"strict":self.strict(),"relative":relative,"waitForAttachment":self.wait_for_attachment,"waitTimeoutMs":timeout_ms});
        Ok(format!(
            "({})=>({})({}, {}, {}, argument)",
            if relative {
                "root,argument"
            } else {
                "argument"
            },
            include_str!("../reading/selector-evaluation.js"),
            injected,
            if relative { "root" } else { "document" },
            options
        ))
    }
}
fn has_reference(locator: &crate::browser::page::Locator) -> bool {
    use crate::browser::page::Locator;
    match locator {
        Locator::Element { .. } => true,
        Locator::Within { parent, child } => has_reference(parent) || has_reference(child),
        Locator::Filter { locator, .. } | Locator::Nth { locator, .. } => has_reference(locator),
        _ => false,
    }
}
pub(crate) fn selector_schema(reference: &Value) -> Value {
    object(
        json!({"operation":{"const":"selector_evaluate"},"scope":reference,"options":object(json!({"locator":{"$ref":"#/$defs/browserLocator"},"expression":{"type":"string","minLength":1,"maxLength":8192},"isFunction":{"type":["boolean","null"]},"argument":argument_schema(),"all":{"type":"boolean","default":false},"strict":{"type":["boolean","null"],"description":"Defaults to true for attachment waits, false for immediate selection; attachment waits require strict selection"},"returnHandle":{"type":"boolean","default":false},"waitForAttachment":{"type":"boolean","default":false},"waitTimeoutMs":{"type":["integer","null"],"minimum":1,"maximum":300000,"description":"Attachment selection deadline; default 30000 ms, capped by the operation deadline; requires waitForAttachment"}}), &["locator","expression"])}),
        &["operation", "scope", "options"],
    )
}
pub(crate) fn capabilities(available: bool) -> Value {
    json!({"available":available,"operation":"selector_evaluate","scopes":["page","frame","element-handle"],"single":"first-match-or-explicit-strict","all":"immediate-array-including-empty","waitForAttachment":"strict-single-current-document","snapshotElementLocators":false,"returnHandle":true})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selector_evaluation_checks_options_and_scopes_element_references() {
        let frame = Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        let mut options: SelectorOptions = serde_json::from_value(json!({"locator":{"kind":"css","selector":".acme"},"expression":"nodes=>nodes.length","all":true})).unwrap();
        assert!(options.validate().is_ok());
        crate::schema::validate(
            &crate::browser::contract::schemas()["request"],
            &json!({"operation":"selector_evaluate","scope":frame,"options":options}),
        )
        .unwrap();
        assert!(
            options
                .native_expression(frame, false)
                .unwrap()
                .starts_with("(argument)=>")
        );
        assert!(
            options
                .native_expression(frame, true)
                .unwrap()
                .starts_with("(root,argument)=>")
        );
        options.strict = Some(true);
        assert_eq!(options.validate(), Err("SELECTOR_EVALUATION_OPTIONS"));
        options.strict = Some(false);
        options.locator = crate::browser::page::Locator::Element {
            frame,
            id: uuid::Uuid::new_v4(),
        };
        assert_eq!(options.validate(), Err("SELECTOR_REFERENCE_UNAVAILABLE"));
        let foreign = Reference {
            generation: uuid::Uuid::new_v4(),
            ..frame
        };
        assert!(options.native_expression(foreign, false).is_err());
        options.locator = crate::browser::page::Locator::Css {
            selector: ".acme".into(),
        };
        options.expression.clear();
        assert_eq!(options.validate(), Err("EXPRESSION_BOUNDS"));
    }
    #[test]
    fn selector_evaluation_wait_defaults_are_strict_and_incompatible_modes_refuse() {
        let base = json!({"locator":{"kind":"css","selector":".acme"},"expression":"element=>element","waitForAttachment":true,"returnHandle":true});
        let options: SelectorOptions = serde_json::from_value(base.clone()).unwrap();
        assert!(options.strict());
        assert!(options.validate().is_ok());
        for extra in [
            json!({"strict":false}),
            json!({"all":true}),
            json!({"waitTimeoutMs":0}),
            json!({"waitTimeoutMs":300001}),
            json!({"waitForAttachment":false,"waitTimeoutMs":5}),
        ] {
            let mut value = base.clone();
            value
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert_eq!(
                serde_json::from_value::<SelectorOptions>(value)
                    .unwrap()
                    .validate(),
                Err("SELECTOR_EVALUATION_OPTIONS")
            );
        }
    }
}
