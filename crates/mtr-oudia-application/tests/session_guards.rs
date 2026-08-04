use mtr_oudia_application::{ConversionSessionStore, SessionId};

#[test]
fn unknown_and_cross_session_ids_are_rejected() {
    let store = ConversionSessionStore::new(2);
    let first = store.create("one".into());
    let second = store.create("two".into());
    assert!(store.remove(&first));
    assert!(!store.remove(&SessionId("missing".into())));
    assert_ne!(first, second);
}

#[test]
fn store_evicts_old_sessions_at_its_bound() {
    let store = ConversionSessionStore::new(1);
    let first = store.create("one".into());
    let second = store.create("two".into());
    assert!(!store.remove(&first));
    assert!(store.remove(&second));
}
