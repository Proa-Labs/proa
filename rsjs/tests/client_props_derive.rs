// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for `#[derive(rsjs::ClientProps)]` (M7f).
//!
//! Positive tests live here; negative (span-pointed compile-error)
//! cases are covered by `compile_fail` doctests on the
//! `rsjs::ClientProps` re-export so they exercise the real proc-macro
//! pipeline at `cargo test` time without needing trybuild.

use rsjs::{ClientDecode, ClientEncode, ClientPropsFieldType, ClientPropsShape};

#[derive(rsjs::ClientProps)]
pub struct UserDraft {
    pub username: String,
    pub email: String,
    pub age: u32,
    pub admin: bool,
}

#[test]
fn user_draft_encode_shape() {
    let u = UserDraft {
        username: "alice".to_string(),
        email: "a@example.com".to_string(),
        age: 30,
        admin: false,
    };
    let s = u.encode();
    assert_eq!(
        s,
        "{\"username\":\"alice\",\"email\":\"a@example.com\",\"age\":30,\"admin\":false}"
    );
}

#[test]
fn user_draft_round_trips_byte_identical() {
    let u = UserDraft {
        username: "bob \"the\\builder\"".to_string(),
        email: "b@example.com".to_string(),
        age: 41,
        admin: true,
    };
    let once = u.encode();
    let decoded = UserDraft::decode(&once).unwrap();
    let twice = decoded.encode();
    assert_eq!(
        once, twice,
        "encode(decode(encode(x))) must match encode(x)"
    );
}

#[test]
fn user_draft_metadata_sibling_const_is_in_scope() {
    // The sibling const must be referenceable at the same module path
    // as the type itself by Rust name resolution. M7e/M7g rely on this.
    let meta: &'static rsjs::ClientPropsMeta = __rsjs_codec_meta_UserDraft;
    assert_eq!(meta.name, "UserDraft");
    assert!(matches!(meta.shape, ClientPropsShape::Object));
    assert_eq!(meta.fields.len(), 4);
    assert_eq!(meta.fields[0].name, "username");
    assert!(matches!(meta.fields[0].ty, ClientPropsFieldType::String));
    assert_eq!(meta.fields[1].name, "email");
    assert!(matches!(meta.fields[1].ty, ClientPropsFieldType::String));
    assert_eq!(meta.fields[2].name, "age");
    assert!(matches!(meta.fields[2].ty, ClientPropsFieldType::U32));
    assert_eq!(meta.fields[3].name, "admin");
    assert!(matches!(meta.fields[3].ty, ClientPropsFieldType::Bool));
}

#[derive(Debug, PartialEq, rsjs::ClientProps)]
#[repr(u8)]
pub enum FulfillmentMode {
    StorePickup = 7,
    Courier = 2,
}

#[derive(Debug, PartialEq, rsjs::ClientProps)]
pub struct DeliveryDraft {
    pub mode: FulfillmentMode,
    pub fallback: Option<FulfillmentMode>,
}

#[test]
fn fieldless_enum_uses_exact_variant_name_tags() {
    assert_eq!(FulfillmentMode::StorePickup.encode(), "\"StorePickup\"");
    assert_eq!(FulfillmentMode::Courier.encode(), "\"Courier\"");
    assert_eq!(
        FulfillmentMode::decode("  \"Courier\" \n").unwrap(),
        FulfillmentMode::Courier
    );

    let error = FulfillmentMode::decode("\"Drone\"").unwrap_err();
    assert_eq!(
        error.msg,
        "unknown fieldless enum tag for `FulfillmentMode`"
    );
}

#[test]
fn fieldless_enum_schema_and_nested_field_metadata_are_typed() {
    let enum_meta = __rsjs_codec_meta_FulfillmentMode;
    assert_eq!(enum_meta.name, "FulfillmentMode");
    assert!(enum_meta.fields.is_empty());
    match enum_meta.shape {
        ClientPropsShape::TaggedString { variants } => {
            assert_eq!(variants, &["StorePickup", "Courier"]);
        }
        ClientPropsShape::Object => panic!("fieldless enum must use a tagged-string schema"),
    }

    let draft_meta = __rsjs_codec_meta_DeliveryDraft;
    assert!(matches!(draft_meta.shape, ClientPropsShape::Object));
    match draft_meta.fields[0].ty {
        ClientPropsFieldType::NestedClientProps(nested) => match nested.shape {
            ClientPropsShape::TaggedString { variants } => {
                assert_eq!(variants, &["StorePickup", "Courier"]);
            }
            ClientPropsShape::Object => panic!("enum field metadata must be tagged-string"),
        },
        _ => panic!("enum field must retain its generated ClientProps schema"),
    }
    match draft_meta.fields[1].ty {
        ClientPropsFieldType::OptionOf(inner) => match inner {
            ClientPropsFieldType::NestedClientProps(nested) => {
                assert!(matches!(
                    nested.shape,
                    ClientPropsShape::TaggedString { .. }
                ));
            }
            _ => panic!("optional enum must retain nested ClientProps metadata"),
        },
        _ => panic!("fallback must be represented as OptionOf"),
    }

    let draft = DeliveryDraft {
        mode: FulfillmentMode::Courier,
        fallback: Some(FulfillmentMode::StorePickup),
    };
    let wire = draft.encode();
    assert_eq!(wire, "{\"mode\":\"Courier\",\"fallback\":\"StorePickup\"}");
    assert_eq!(DeliveryDraft::decode(&wire).unwrap(), draft);
}

// Wide-int + Option + Vec + nested struct: cover the rest of the
// vocabulary in a second fixture so the round-trip + metadata story is
// exercised end-to-end.
#[derive(rsjs::ClientProps)]
pub struct Tag {
    pub label: String,
}

#[derive(rsjs::ClientProps)]
pub struct Account {
    pub balance_cents: u64,
    pub bonus: Option<i64>,
    pub tags: Vec<Tag>,
    pub primary_tag: Option<Tag>,
}

#[test]
fn account_encodes_wide_ints_as_strings() {
    let a = Account {
        balance_cents: 12345678901234567890,
        bonus: Some(-42),
        tags: vec![
            Tag {
                label: "vip".to_string(),
            },
            Tag {
                label: "early".to_string(),
            },
        ],
        primary_tag: None,
    };
    let s = a.encode();
    // Wide ints are JSON strings; nested structs and Option<None> use
    // their declared shapes.
    assert!(s.contains("\"balance_cents\":\"12345678901234567890\""));
    assert!(s.contains("\"bonus\":\"-42\""));
    assert!(s.contains("\"tags\":[{\"label\":\"vip\"},{\"label\":\"early\"}]"));
    assert!(s.contains("\"primary_tag\":null"));

    // Round-trip is byte-identical.
    let back = Account::decode(&s).unwrap();
    let again = back.encode();
    assert_eq!(s, again);
}

#[test]
fn account_metadata_uses_wide_int_and_nested_variants() {
    let meta = __rsjs_codec_meta_Account;
    assert_eq!(meta.fields.len(), 4);
    assert!(matches!(meta.fields[0].ty, ClientPropsFieldType::U64));
    match meta.fields[1].ty {
        ClientPropsFieldType::OptionOf(inner) => {
            assert!(matches!(inner, ClientPropsFieldType::I64));
        }
        _ => panic!("expected OptionOf(I64) for `bonus`"),
    }
    match meta.fields[2].ty {
        ClientPropsFieldType::VecOf(inner) => match inner {
            ClientPropsFieldType::NestedClientProps(nested) => {
                assert_eq!(nested.name, "Tag");
                assert_eq!(nested.fields.len(), 1);
                assert_eq!(nested.fields[0].name, "label");
            }
            _ => panic!("expected VecOf(NestedClientProps) for `tags`"),
        },
        _ => panic!("expected VecOf for `tags`"),
    }
    match meta.fields[3].ty {
        ClientPropsFieldType::OptionOf(inner) => match inner {
            ClientPropsFieldType::NestedClientProps(nested) => {
                assert_eq!(nested.name, "Tag");
            }
            _ => panic!("expected OptionOf(NestedClientProps) for `primary_tag`"),
        },
        _ => panic!("expected OptionOf for `primary_tag`"),
    }
}

#[test]
fn account_decode_rejects_unknown_field() {
    let s = "{\"balance_cents\":\"1\",\"bonus\":null,\"tags\":[],\"primary_tag\":null,\"x\":1}";
    let r = Account::decode(s);
    assert!(r.is_err(), "extra fields must error");
}

#[test]
fn account_decode_rejects_missing_field() {
    let s = "{\"balance_cents\":\"1\",\"bonus\":null,\"tags\":[]}";
    let r = Account::decode(s);
    assert!(r.is_err(), "missing fields must error");
}

#[test]
fn account_decode_rejects_duplicate_field() {
    let s = "{\"balance_cents\":\"1\",\"balance_cents\":\"2\",\"bonus\":null,\"tags\":[],\"primary_tag\":null}";
    let r = Account::decode(s);
    assert!(r.is_err(), "duplicate fields must error");
}

// Empty struct: a zero-field named struct is valid (just `{}`).
#[derive(rsjs::ClientProps)]
pub struct Empty {}

#[test]
fn empty_struct_round_trips() {
    let e = Empty {};
    let s = e.encode();
    assert_eq!(s, "{}");
    let _back = Empty::decode("{}").unwrap();
    let _back2 = Empty::decode("{ }").unwrap();
}

// Reproduces the path-resolution scenario where the user has shadowed
// the `rsjs` name with a local module. The derive must emit absolute
// `::rsjs::...` paths so generated code still resolves correctly even
// when `rsjs::` would resolve to something else in the local scope.
mod shadowed_crate_scenario {
    // A local `rsjs` module that does NOT contain the codec items the
    // derive references. If the derive emitted relative `rsjs::...`
    // paths they would resolve to this module and fail to compile.
    // With absolute `::rsjs::...` paths, the generated impl bypasses
    // this shadow and reaches the real crate.
    #[allow(dead_code)]
    mod rsjs {
        pub struct NotTheRealCrate;
    }

    // Use the absolute path on the derive itself so the proc-macro
    // is found; the macro's *output* is what we're testing for path
    // hygiene against the local `rsjs` shadow above.
    #[derive(::rsjs::ClientProps)]
    pub struct UnderShadow {
        pub name: String,
        pub count: u32,
    }

    #[test]
    fn under_shadow_round_trips() {
        use ::rsjs::{ClientDecode, ClientEncode};
        let v = UnderShadow {
            name: "alice".to_string(),
            count: 7,
        };
        let s = v.encode();
        let back = UnderShadow::decode(&s).unwrap();
        assert_eq!(back.name, "alice");
        assert_eq!(back.count, 7);
    }
}

// Reproduces the path-resolution scenario where the user has renamed
// the `rsjs` crate via `pub use rsjs as foo`. The derive must work
// when invoked through the alias.
mod renamed_crate_scenario {
    pub use ::rsjs as renamed_rsjs;

    #[derive(renamed_rsjs::ClientProps)]
    pub struct UnderRename {
        pub name: String,
        pub count: u32,
    }

    #[test]
    fn under_rename_round_trips() {
        use renamed_rsjs::{ClientDecode, ClientEncode};
        let v = UnderRename {
            name: "alice".to_string(),
            count: 7,
        };
        let s = v.encode();
        let back = UnderRename::decode(&s).unwrap();
        assert_eq!(back.name, "alice");
        assert_eq!(back.count, 7);
    }
}

#[derive(rsjs::ClientProps)]
pub struct StaticOption {
    pub value: &'static str,
    pub label: &'static str,
    pub disabled: bool,
}

#[test]
#[allow(clippy::needless_borrow)] // Verifies the derived reference implementation.
fn borrowed_static_option_encodes_owned_and_borrowed_values() {
    let option = StaticOption {
        value: "apple",
        label: "Apple",
        disabled: false,
    };

    assert_eq!(
        option.encode(),
        "{\"value\":\"apple\",\"label\":\"Apple\",\"disabled\":false}"
    );
    assert_eq!(
        (&option).encode(),
        "{\"value\":\"apple\",\"label\":\"Apple\",\"disabled\":false}"
    );
}

#[derive(rsjs::ClientProps)]
pub struct StaticOptionGroup<'a> {
    pub title: &'static str,
    pub options: &'a [StaticOption],
}

#[test]
#[allow(clippy::needless_borrow)] // Verifies encoding through a borrowed slice.
fn lifetime_struct_encodes_borrowed_slices() {
    let options = [
        StaticOption {
            value: "apple",
            label: "Apple",
            disabled: false,
        },
        StaticOption {
            value: "pear",
            label: "Pear",
            disabled: true,
        },
    ];
    let group = StaticOptionGroup {
        title: "Fruit",
        options: &options,
    };

    assert_eq!(
        group.encode(),
        "{\"title\":\"Fruit\",\"options\":[{\"value\":\"apple\",\"label\":\"Apple\",\"disabled\":false},{\"value\":\"pear\",\"label\":\"Pear\",\"disabled\":true}]}"
    );
    assert_eq!(
        (&options[..]).encode(),
        "[{\"value\":\"apple\",\"label\":\"Apple\",\"disabled\":false},{\"value\":\"pear\",\"label\":\"Pear\",\"disabled\":true}]"
    );
}
