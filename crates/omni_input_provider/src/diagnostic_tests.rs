use eyre::WrapErr;
use omni_diagnostic::{Audience, find};
use value_bag::ValueBag;

use crate::error::Error;

fn schema_error(
    inner: omni_input_schema::error::ErrorInner,
) -> omni_input_schema::Error {
    omni_input_schema::Error::from(inner)
}

fn report(error: Error) -> eyre::Report {
    Err::<(), _>(error)
        .wrap_err("collecting inputs")
        .unwrap_err()
}

#[test]
fn schema_errors_keep_their_codes_through_the_provider_error() {
    let r = report(
        schema_error(
            omni_input_schema::error::ErrorInner::SecretRememberConflict {
                input_name: "api_key".into(),
            },
        )
        .into(),
    );
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "input.secret_remember_conflict");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["input_name"], "api_key");
}

#[test]
fn invalid_value_keeps_its_details_and_hides_the_value() {
    let r = report(
        schema_error(omni_input_schema::error::ErrorInner::InvalidValue {
            input_name: "port".into(),
            value: ValueBag::from_str("hunter2").to_owned(),
            error_message: "must be below 1024".into(),
        })
        .into(),
    );
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "input.invalid_value");
    assert_eq!(d.details().unwrap()["input_name"], "port");
    assert!(!d.details().unwrap().to_string().contains("hunter2"));
}

#[test]
fn duplicate_input_names_are_found_through_the_provider_error() {
    let r = report(
        schema_error(omni_input_schema::error::ErrorInner::DuplicateInputName(
            "token".into(),
        ))
        .into(),
    );
    assert_eq!(find(&r).expect("diagnostic").code(), "input.duplicate_name");
}

#[test]
fn plain_provider_errors_stay_uncoded() {
    let r = report(Error::custom(eyre::eyre!("boom")));
    assert!(find(&r).is_none());
}
