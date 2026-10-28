use crate::ContractError;
use serde::de::{
    DeserializeOwned, DeserializeSeed, EnumAccess, Error, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use std::cell::RefCell;
use std::fmt;

pub(super) fn deserialize<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ContractError> {
    let fault = RefCell::new(None);
    let mut reader = serde_json::Deserializer::from_slice(bytes);
    match T::deserialize(Tracked {
        inner: &mut reader,
        fault: &fault,
    }) {
        Ok(value) => {
            reader.end()?;
            Ok(value)
        }
        Err(error) => Err(fault.into_inner().unwrap_or(ContractError::Json(error))),
    }
}

// Carry native model faults through Serde without error-string matching or global state.
struct Tracked<'a, T> {
    inner: T,
    fault: &'a RefCell<Option<ContractError>>,
}

macro_rules! deserialize_method {
    ($method:ident $(, $arg:ident : $kind:ty)*) => {
        fn $method<V: Visitor<'de>>(self, $($arg: $kind,)* visitor: V) -> Result<V::Value, Self::Error> {
            self.inner.$method($($arg,)* Tracked { inner: visitor, fault: self.fault })
        }
    };
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Tracked<'_, D> {
    type Error = D::Error;

    deserialize_method!(deserialize_any);
    deserialize_method!(deserialize_bool);
    deserialize_method!(deserialize_i8);
    deserialize_method!(deserialize_i16);
    deserialize_method!(deserialize_i32);
    deserialize_method!(deserialize_i64);
    deserialize_method!(deserialize_i128);
    deserialize_method!(deserialize_u8);
    deserialize_method!(deserialize_u16);
    deserialize_method!(deserialize_u32);
    deserialize_method!(deserialize_u64);
    deserialize_method!(deserialize_u128);
    deserialize_method!(deserialize_f32);
    deserialize_method!(deserialize_f64);
    deserialize_method!(deserialize_char);
    deserialize_method!(deserialize_str);
    deserialize_method!(deserialize_string);
    deserialize_method!(deserialize_bytes);
    deserialize_method!(deserialize_byte_buf);
    deserialize_method!(deserialize_option);
    deserialize_method!(deserialize_unit);
    deserialize_method!(deserialize_unit_struct, name: &'static str);
    deserialize_method!(deserialize_seq);
    deserialize_method!(deserialize_tuple, len: usize);
    deserialize_method!(deserialize_tuple_struct, name: &'static str, len: usize);
    deserialize_method!(deserialize_map);
    deserialize_method!(deserialize_struct, name: &'static str, fields: &'static [&'static str]);
    deserialize_method!(deserialize_enum, name: &'static str, variants: &'static [&'static str]);
    deserialize_method!(deserialize_identifier);
    deserialize_method!(deserialize_ignored_any);

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if name != "EvidenceSource" {
            return self.inner.deserialize_newtype_struct(
                name,
                Tracked {
                    inner: visitor,
                    fault: self.fault,
                },
            );
        }
        let raw = <&'de RawValue>::deserialize(self.inner)?;
        if raw.get().starts_with('{') {
            let members: indexmap::IndexMap<String, &RawValue> =
                serde_json::from_str(raw.get()).map_err(D::Error::custom)?;
            let first = members.first().map(|(name, _)| name.as_str());
            if matches!(first, Some("$id" | "$ref" | "$values")) {
                return Err(D::Error::custom(
                    "unsupported evidence source metadata property",
                ));
            }
            if first != Some("kind") {
                *self.fault.borrow_mut() = Some(ContractError::MissingTypeDiscriminator);
                return Err(D::Error::custom("evidence source requires kind first"));
            }
        }
        let mut reader = serde_json::Deserializer::from_str(raw.get());
        visitor
            .visit_newtype_struct(Tracked {
                inner: &mut reader,
                fault: self.fault,
            })
            .map_err(D::Error::custom)
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Tracked<'_, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.inner.deserialize(Tracked {
            inner: deserializer,
            fault: self.fault,
        })
    }
}

macro_rules! visit_method {
    ($method:ident, $kind:ty) => {
        fn $method<E: Error>(self, value: $kind) -> Result<Self::Value, E> {
            self.inner.$method(value)
        }
    };
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Tracked<'_, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.expecting(formatter)
    }

    visit_method!(visit_bool, bool);
    visit_method!(visit_i8, i8);
    visit_method!(visit_i16, i16);
    visit_method!(visit_i32, i32);
    visit_method!(visit_i64, i64);
    visit_method!(visit_i128, i128);
    visit_method!(visit_u8, u8);
    visit_method!(visit_u16, u16);
    visit_method!(visit_u32, u32);
    visit_method!(visit_u64, u64);
    visit_method!(visit_u128, u128);
    visit_method!(visit_f32, f32);
    visit_method!(visit_f64, f64);
    visit_method!(visit_char, char);
    visit_method!(visit_str, &str);
    visit_method!(visit_borrowed_str, &'de str);
    visit_method!(visit_string, String);
    visit_method!(visit_bytes, &[u8]);
    visit_method!(visit_borrowed_bytes, &'de [u8]);
    visit_method!(visit_byte_buf, Vec<u8>);

    fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_none()
    }
    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.inner.visit_some(Tracked {
            inner: deserializer,
            fault: self.fault,
        })
    }
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        self.inner.visit_newtype_struct(Tracked {
            inner: deserializer,
            fault: self.fault,
        })
    }
    fn visit_seq<A: SeqAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_seq(Tracked {
            inner: access,
            fault: self.fault,
        })
    }
    fn visit_map<A: MapAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_map(Tracked {
            inner: access,
            fault: self.fault,
        })
    }
    fn visit_enum<A: EnumAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_enum(Tracked {
            inner: access,
            fault: self.fault,
        })
    }
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Tracked<'_, A> {
    type Error = A::Error;
    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        self.inner.next_element_seed(Tracked {
            inner: seed,
            fault: self.fault,
        })
    }
    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Tracked<'_, A> {
    type Error = A::Error;
    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        self.inner.next_key_seed(Tracked {
            inner: seed,
            fault: self.fault,
        })
    }
    fn next_value_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<S::Value, Self::Error> {
        self.inner.next_value_seed(Tracked {
            inner: seed,
            fault: self.fault,
        })
    }
    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

impl<'a, 'de, A: EnumAccess<'de>> EnumAccess<'de> for Tracked<'a, A> {
    type Error = A::Error;
    type Variant = Tracked<'a, A::Variant>;
    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Self::Variant), Self::Error> {
        let (value, variant) = self.inner.variant_seed(Tracked {
            inner: seed,
            fault: self.fault,
        })?;
        Ok((
            value,
            Tracked {
                inner: variant,
                fault: self.fault,
            },
        ))
    }
}

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Tracked<'_, A> {
    type Error = A::Error;
    fn unit_variant(self) -> Result<(), Self::Error> {
        self.inner.unit_variant()
    }
    fn newtype_variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<S::Value, Self::Error> {
        self.inner.newtype_variant_seed(Tracked {
            inner: seed,
            fault: self.fault,
        })
    }
    fn tuple_variant<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.tuple_variant(
            len,
            Tracked {
                inner: visitor,
                fault: self.fault,
            },
        )
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.struct_variant(
            fields,
            Tracked {
                inner: visitor,
                fault: self.fault,
            },
        )
    }
}
