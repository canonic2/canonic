use super::*;
use crate::configuration::Candidate;

#[test]
fn project_composition_requires_one_complete_candidate() {
    let lock = Candidate::from_yaml("schema_version: 1\npacks: []\n")
        .unwrap()
        .capture(&[], None)
        .unwrap()
        .lock()
        .clone();
    let input = (Scope::Project, lock);
    let effective = compose(std::slice::from_ref(&input)).unwrap();
    assert_eq!(effective.selections().count(), 0);
    assert_eq!(
        effective.revision().unwrap(),
        compose(std::slice::from_ref(&input))
            .unwrap()
            .revision()
            .unwrap()
    );
    assert!(compose(&[]).is_err());
    assert!(compose(&[input.clone(), input]).is_err());
    for scope in ["user", "organization"] {
        assert!(serde_json::from_value::<Scope>(serde_json::json!(scope)).is_err());
    }
}
