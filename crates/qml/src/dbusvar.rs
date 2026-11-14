//! Bounded decoding of `v` values from untrusted D-Bus peers. zvariant's `Value` keeps one enum
//! per element (an `ay` of N bytes costs ~N×24 bytes), so a 100 MB notification image would
//! cost gigabytes. Images and icon pixmaps are decoded typed into plain byte vectors; any other
//! variant goes through a value budget and is skipped, without allocating, when it is larger.

use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::fmt;
use zbus::zvariant::{
    Array, Dict, OwnedObjectPath, OwnedValue, Signature, StructureBuilder, Type, Value,
};

/// Values (container elements included) one generic variant may decode to.
pub(crate) const MAX_VALUES: usize = 1 << 16;

/// Longest side a picture received over D-Bus is kept at (4 MiB of pixels): a notification's
/// image or a tray pixmap up to it is kept as it came, as Quickshell keeps it; a bigger one is
/// averaged down once when it arrives (docs/quickshell-differences.md).
pub(crate) const MAX_SIDE: usize = 1024;

/// An `ay` read in one piece.
#[derive(Default, Serialize)]
pub(crate) struct Bytes(pub Vec<u8>);

impl Type for Bytes {
    const SIGNATURE: &'static Signature = <Vec<u8> as Type>::SIGNATURE;
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Bytes;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a byte array")
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Bytes, E> {
                Ok(Bytes(v.to_vec()))
            }
            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(v))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Bytes, A::Error> {
                let mut v = Vec::new();
                while let Some(b) = s.next_element::<u8>()? {
                    v.push(b);
                }
                Ok(Bytes(v))
            }
        }
        d.deserialize_byte_buf(V)
    }
}

/// `(iiibiiay)` notification `image-data`.
pub(crate) struct ImageData {
    pub w: i32,
    pub h: i32,
    pub stride: i32,
    pub alpha: bool,
    pub bits: i32,
    pub chans: i32,
    pub data: Vec<u8>,
}

/// One `(iiay)` StatusNotifierItem pixmap: ARGB32 in network byte order.
pub(crate) type RawPix = (i32, i32, Bytes);

/// A received `v`.
pub(crate) enum Var {
    Value(OwnedValue),
    Image(ImageData),
    /// `a(iiay)`.
    Pixmaps(Vec<RawPix>),
    /// `(sa(iiay)ss)` (icon name, pixmaps, title, description): title and description.
    ToolTip(String, String),
    /// Over [`MAX_VALUES`], or a type without a value (fd).
    Skipped,
}

impl Type for Var {
    const SIGNATURE: &'static Signature = &Signature::Variant;
}

impl<'de> Deserialize<'de> for Var {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_seq(VarV)
    }
}

fn need<T, E: de::Error>(v: Option<T>) -> Result<T, E> {
    v.ok_or_else(|| E::custom("variant without a value"))
}

struct VarV;

impl<'de> Visitor<'de> for VarV {
    type Value = Var;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a variant")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Var, A::Error> {
        let sig: Signature = need(s.next_element()?)?;
        Ok(match sig.to_string().as_str() {
            "(iiibiiay)" => {
                let (w, h, stride, alpha, bits, chans, Bytes(data)) = need(s.next_element()?)?;
                Var::Image(ImageData {
                    w,
                    h,
                    stride,
                    alpha,
                    bits,
                    chans,
                    data,
                })
            }
            "a(iiay)" => Var::Pixmaps(need(s.next_element()?)?),
            "(sa(iiay)ss)" => {
                let (_, _, title, desc): (String, Vec<RawPix>, String, String) =
                    need(s.next_element()?)?;
                Var::ToolTip(title, desc)
            }
            _ => {
                let budget = Cell::new(MAX_VALUES);
                let v = need(s.next_element_seed(Dyn {
                    sig: &sig,
                    budget: &budget,
                })?)?;
                match v.map(OwnedValue::try_from) {
                    Some(Ok(v)) => Var::Value(v),
                    _ => Var::Skipped,
                }
            }
        })
    }
}

/// One value of signature `sig`; `None` once the budget is spent (the rest is skipped).
struct Dyn<'a> {
    sig: &'a Signature,
    budget: &'a Cell<usize>,
}

impl<'de> DeserializeSeed<'de> for Dyn<'_> {
    type Value = Option<Value<'static>>;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        let left = self.budget.get();
        if left == 0 {
            IgnoredAny::deserialize(d)?;
            return Ok(None);
        }
        self.budget.set(left - 1);
        let budget = self.budget;
        Ok(Some(match self.sig {
            Signature::U8 => u8::deserialize(d)?.into(),
            Signature::Bool => bool::deserialize(d)?.into(),
            Signature::I16 => i16::deserialize(d)?.into(),
            Signature::U16 => u16::deserialize(d)?.into(),
            Signature::I32 => i32::deserialize(d)?.into(),
            Signature::U32 => u32::deserialize(d)?.into(),
            Signature::I64 => i64::deserialize(d)?.into(),
            Signature::U64 => u64::deserialize(d)?.into(),
            Signature::F64 => f64::deserialize(d)?.into(),
            Signature::Str => String::deserialize(d)?.into(),
            Signature::ObjectPath => {
                Value::ObjectPath(OwnedObjectPath::deserialize(d)?.into_inner())
            }
            Signature::Signature => Value::Signature(Signature::deserialize(d)?),
            Signature::Variant => {
                return Ok(d
                    .deserialize_seq(NestedV(budget))?
                    .map(|v| Value::Value(Box::new(v))));
            }
            Signature::Array(c) if *c.signature() == Signature::U8 => {
                let Bytes(b) = Bytes::deserialize(d)?;
                let Some(rest) = budget.get().checked_sub(b.len()) else {
                    budget.set(0);
                    return Ok(None);
                };
                budget.set(rest);
                let mut a = Array::new(&Signature::U8);
                for x in b {
                    a.append(Value::U8(x)).map_err(de::Error::custom)?;
                }
                Value::Array(a)
            }
            Signature::Array(c) => {
                return d.deserialize_seq(ArrV(c.signature(), budget));
            }
            Signature::Dict { key, value } => {
                return d.deserialize_map(DictV(key.signature(), value.signature(), budget));
            }
            Signature::Structure(f) => {
                let f: Vec<&Signature> = f.iter().collect();
                return d.deserialize_seq(StructV(f, budget));
            }
            _ => {
                IgnoredAny::deserialize(d)?;
                return Ok(None);
            }
        }))
    }
}

struct NestedV<'a>(&'a Cell<usize>);

impl<'de> Visitor<'de> for NestedV<'_> {
    type Value = Option<Value<'static>>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a variant")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Self::Value, A::Error> {
        let sig: Signature = need(s.next_element()?)?;
        need(s.next_element_seed(Dyn {
            sig: &sig,
            budget: self.0,
        })?)
    }
}

struct ArrV<'a>(&'a Signature, &'a Cell<usize>);

impl<'de> Visitor<'de> for ArrV<'_> {
    type Value = Option<Value<'static>>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Self::Value, A::Error> {
        let mut a = Some(Array::new(self.0));
        while let Some(e) = s.next_element_seed(Dyn {
            sig: self.0,
            budget: self.1,
        })? {
            match (e, a.as_mut()) {
                (Some(e), Some(arr)) => arr.append(e).map_err(de::Error::custom)?,
                _ => a = None,
            }
        }
        Ok(a.map(Value::Array))
    }
}

struct DictV<'a>(&'a Signature, &'a Signature, &'a Cell<usize>);

impl<'de> Visitor<'de> for DictV<'_> {
    type Value = Option<Value<'static>>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a dict")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
        let mut dict = Some(Dict::new(self.0, self.1));
        while let Some(k) = m.next_key_seed(Dyn {
            sig: self.0,
            budget: self.2,
        })? {
            let v = m.next_value_seed(Dyn {
                sig: self.1,
                budget: self.2,
            })?;
            match (k, v, dict.as_mut()) {
                (Some(k), Some(v), Some(d)) => d.append(k, v).map_err(de::Error::custom)?,
                _ => dict = None,
            }
        }
        Ok(dict.map(Value::Dict))
    }
}

struct StructV<'a>(Vec<&'a Signature>, &'a Cell<usize>);

impl<'de> Visitor<'de> for StructV<'_> {
    type Value = Option<Value<'static>>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a structure")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Self::Value, A::Error> {
        let mut b = Some(StructureBuilder::new());
        for sig in self.0 {
            let v = need(s.next_element_seed(Dyn {
                sig,
                budget: self.1,
            })?)?;
            b = match (v, b) {
                (Some(v), Some(b)) => Some(b.append_field(v)),
                _ => None,
            };
        }
        match b {
            Some(b) => Ok(Some(Value::Structure(
                b.build().map_err(de::Error::custom)?,
            ))),
            None => Ok(None),
        }
    }
}

/// Premultiplied RGBA of a `w`×`h` image whose premultiplied pixel at (x, y) is `px(x, y)`,
/// area-averaged so that no side exceeds `max` (aspect kept). Walks each source pixel once.
pub(crate) fn fit(
    w: usize,
    h: usize,
    max: usize,
    px: impl Fn(usize, usize) -> [u8; 4],
) -> (usize, usize, Vec<[u8; 4]>) {
    if w <= max && h <= max {
        let mut out = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                out.push(px(x, y));
            }
        }
        return (w, h, out);
    }
    let (tw, th) = if w >= h {
        (max, (h * max).div_ceil(w).max(1))
    } else {
        ((w * max).div_ceil(h).max(1), max)
    };
    let mut out = Vec::with_capacity(tw * th);
    let mut acc = vec![[0u32; 4]; tw];
    for oy in 0..th {
        let (y0, y1) = (oy * h / th, ((oy + 1) * h / th).max(oy * h / th + 1));
        acc.iter_mut().for_each(|a| *a = [0; 4]);
        for y in y0..y1 {
            for (ox, a) in acc.iter_mut().enumerate() {
                let (x0, x1) = (ox * w / tw, ((ox + 1) * w / tw).max(ox * w / tw + 1));
                for x in x0..x1 {
                    let p = px(x, y);
                    for k in 0..4 {
                        a[k] += p[k] as u32;
                    }
                }
            }
        }
        for (ox, a) in acc.iter().enumerate() {
            let (x0, x1) = (ox * w / tw, ((ox + 1) * w / tw).max(ox * w / tw + 1));
            let n = ((x1 - x0) * (y1 - y0)) as u32;
            out.push(a.map(|c| ((c + n / 2) / n) as u8));
        }
    }
    (tw, th, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use zbus::zvariant::serialized::{Context, Format};
    use zbus::zvariant::{LE, to_bytes};

    fn decode(v: &HashMap<&str, Value>) -> HashMap<String, Var> {
        let ctx = Context::new(Format::DBus, LE, 0);
        let b = to_bytes(ctx, v).unwrap();
        b.deserialize::<HashMap<String, Var>>().unwrap().0
    }

    #[test]
    fn generic_values_round_trip_and_oversized_ones_are_skipped() {
        let mut m: HashMap<&str, Value> = HashMap::new();
        m.insert("s", "x".into());
        m.insert("i", 7i32.into());
        m.insert("l", vec!["a", "b"].into());
        m.insert("big", vec![0u32; MAX_VALUES + 1].into());
        m.insert("bigay", vec![0u8; MAX_VALUES + 1].into());
        m.insert("ay", vec![1u8, 2].into());
        let mut inner: HashMap<&str, Value> = HashMap::new();
        inner.insert("k", Value::Value(Box::new(true.into())));
        m.insert("d", inner.into());
        let got = decode(&m);
        let val = |k: &str| match &got[k] {
            Var::Value(v) => Value::from(v.try_clone().unwrap()),
            _ => panic!("{k} not a value"),
        };
        assert_eq!(val("s"), Value::from("x"));
        assert_eq!(val("i"), Value::from(7i32));
        assert_eq!(val("l"), Value::from(vec!["a", "b"]));
        assert_eq!(val("ay"), Value::from(vec![1u8, 2]));
        assert_eq!(val("d").to_string(), m["d"].to_string());
        assert!(matches!(got["big"], Var::Skipped));
        assert!(matches!(got["bigay"], Var::Skipped));
    }

    #[test]
    fn images_and_pixmaps_decode_typed() {
        let mut m: HashMap<&str, Value> = HashMap::new();
        m.insert(
            "image-data",
            (
                2i32,
                1i32,
                8i32,
                true,
                8i32,
                4i32,
                vec![1u8, 2, 3, 4, 5, 6, 7, 8],
            )
                .into(),
        );
        m.insert("IconPixmap", vec![(1i32, 1i32, vec![9u8, 8, 7, 6])].into());
        m.insert(
            "ToolTip",
            ("n", vec![(1i32, 1i32, vec![0u8; 4])], "t", "d").into(),
        );
        let got = decode(&m);
        let Var::Image(i) = &got["image-data"] else {
            panic!()
        };
        assert_eq!((i.w, i.h, i.stride, i.alpha, i.chans), (2, 1, 8, true, 4));
        assert_eq!(i.data, [1, 2, 3, 4, 5, 6, 7, 8]);
        let Var::Pixmaps(p) = &got["IconPixmap"] else {
            panic!()
        };
        assert_eq!((p[0].0, p[0].1, &p[0].2.0[..]), (1, 1, &[9u8, 8, 7, 6][..]));
        let Var::ToolTip(t, d) = &got["ToolTip"] else {
            panic!()
        };
        assert_eq!((t.as_str(), d.as_str()), ("t", "d"));
    }

    #[test]
    fn fit_averages_down_to_max_side() {
        let (w, h, px) = fit(
            4,
            2,
            2,
            |x, _| if x < 2 { [200, 0, 0, 200] } else { [0; 4] },
        );
        assert_eq!((w, h), (2, 1));
        assert_eq!(px, vec![[200, 0, 0, 200], [0; 4]]);
        let (w, h, px) = fit(3, 1, 4, |x, _| [x as u8; 4]);
        assert_eq!((w, h, px.len()), (3, 1, 3));
        assert_eq!(fit(1000, 10, 256, |_, _| [1; 4]).0, 256);
        assert_eq!(fit(10, 1000, 256, |_, _| [1; 4]).0, 3);
    }
}
