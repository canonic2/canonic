//! Browser credential admission, redaction and private broker fixtures.
use super::*;

#[cfg(unix)]
mod broker;

const PASSWORD: &str = "acme-synthetic-password-7f3a";
const ORIGIN: &str = "https://app.example.com";

fn css(selector: &str) -> Locator {
    Locator::Css {
        selector: selector.into(),
    }
}
fn journey() -> Journey {
    Journey {
        origin: ORIGIN.into(),
        username: Some(css("#username")),
        password: css("#password"),
        submit: css("#submit"),
        signed_in: css("#account"),
        signed_out: css("#sign-in"),
        failure: Some(css("[role=alert]")),
        otp: None,
        otp_submit: None,
        identity: Some(css("#account")),
    }
}
fn reference() -> Reference {
    Reference {
        connection_id: uuid::Uuid::new_v4(),
        id: uuid::Uuid::new_v4(),
        generation: uuid::Uuid::new_v4(),
    }
}

#[test]
fn journeys_refuse_element_references_and_partial_one_time_code_steps() {
    journey().validate().unwrap();
    let mut element = journey();
    element.submit = Locator::Within {
        parent: Box::new(css("form")),
        child: Box::new(Locator::Element {
            frame: reference(),
            id: uuid::Uuid::new_v4(),
        }),
    };
    assert_eq!(element.validate(), Err("JOURNEY_ELEMENT_REFERENCE"));
    let mut otp = journey();
    otp.otp = Some(css("#code"));
    assert_eq!(otp.validate(), Err("JOURNEY_OTP_INCOMPLETE"));
    let mut origin = journey();
    origin.origin = "https://app.example.com/path".into();
    assert!(origin.validate().is_err());
    assert_eq!(
        validate_name("Acme", "INVALID_BROKER_NAME"),
        Err("INVALID_BROKER_NAME")
    );
    assert_eq!(format!("{:?}", Secret(PASSWORD.into())), "Secret(..)");
}
