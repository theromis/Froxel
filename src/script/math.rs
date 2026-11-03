//! Math and array functions for scripts beyond the first set in `execute`.
//!
//! Numbers may be integers or floats. `min`, `max`, `clamp`, and `pow` keep
//! an integer result when every argument is an integer.

use rhai::{Array, Dynamic, Engine, EvalAltResult, FLOAT, INT};

use super::{runtime, MAX_ARRAY};

type Fallible<T> = Result<T, Box<EvalAltResult>>;

pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("PI", || std::f64::consts::PI);
    engine.register_fn("to_float", |value: FLOAT| value);
    engine.register_fn("to_int", |value: INT| value);

    engine.register_fn("sqrt", |value: Dynamic| -> Fallible<FLOAT> {
        let value = float(&value, "sqrt")?;
        if value < 0.0 {
            return Err(runtime("sqrt of a negative number"));
        }
        Ok(value.sqrt())
    });
    engine.register_fn("tan", |value: Dynamic| -> Fallible<FLOAT> {
        Ok(float(&value, "tan")?.tan())
    });
    // The float forms of these are registered in `execute`.
    engine.register_fn("sin", |value: INT| (value as FLOAT).sin());
    engine.register_fn("cos", |value: INT| (value as FLOAT).cos());
    engine.register_fn("floor", |value: INT| value);
    engine.register_fn("ceil", |value: FLOAT| value.ceil());
    engine.register_fn("ceil", |value: INT| value);
    engine.register_fn("round", |value: FLOAT| value.round());
    engine.register_fn("round", |value: INT| value);

    engine.register_fn("min", |a: Dynamic, b: Dynamic| -> Fallible<Dynamic> {
        pair(&a, &b, "min", |a, b| a.min(b), |a, b| a.min(b))
    });
    engine.register_fn("max", |a: Dynamic, b: Dynamic| -> Fallible<Dynamic> {
        pair(&a, &b, "max", |a, b| a.max(b), |a, b| a.max(b))
    });
    engine.register_fn(
        "clamp",
        |value: Dynamic, lo: Dynamic, hi: Dynamic| -> Fallible<Dynamic> {
            if let (Ok(v), Ok(l), Ok(h)) = (value.as_int(), lo.as_int(), hi.as_int()) {
                if l > h {
                    return Err(runtime("clamp range is empty"));
                }
                return Ok(Dynamic::from_int(v.clamp(l, h)));
            }
            let (v, l, h) = (
                float(&value, "clamp")?,
                float(&lo, "clamp")?,
                float(&hi, "clamp")?,
            );
            if !(l <= h) {
                return Err(runtime("clamp range is empty"));
            }
            Ok(Dynamic::from_float(v.clamp(l, h)))
        },
    );
    engine.register_fn("pow", |base: Dynamic, exp: Dynamic| -> Fallible<Dynamic> {
        if let (Ok(b), Ok(e)) = (base.as_int(), exp.as_int()) {
            if e >= 0 {
                let e = u32::try_from(e).map_err(|_| runtime("pow overflows an integer"))?;
                return b
                    .checked_pow(e)
                    .map(Dynamic::from_int)
                    .ok_or_else(|| runtime("pow overflows an integer"));
            }
        }
        Ok(Dynamic::from_float(
            float(&base, "pow")?.powf(float(&exp, "pow")?),
        ))
    });

    engine.register_fn(
        "push",
        |array: &mut Array, value: Dynamic| -> Fallible<()> {
            grow(array)?;
            array.push(value);
            Ok(())
        },
    );
    engine.register_fn("pop", |array: &mut Array| {
        array.pop().unwrap_or(Dynamic::UNIT)
    });
    engine.register_fn(
        "insert",
        |array: &mut Array, index: INT, value: Dynamic| -> Fallible<()> {
            grow(array)?;
            array.insert(position(array.len(), index), value);
            Ok(())
        },
    );
    engine.register_fn("remove", |array: &mut Array, index: INT| {
        let len = array.len() as INT;
        let index = if index < 0 { index + len } else { index };
        if (0..len).contains(&index) {
            array.remove(index as usize)
        } else {
            Dynamic::UNIT
        }
    });
}

fn float(value: &Dynamic, function: &str) -> Fallible<FLOAT> {
    value
        .as_float()
        .or_else(|_| value.as_int().map(|int| int as FLOAT))
        .map_err(|_| runtime(format!("{function} expects numbers")))
}

fn pair(
    a: &Dynamic,
    b: &Dynamic,
    function: &str,
    ints: fn(INT, INT) -> INT,
    floats: fn(FLOAT, FLOAT) -> FLOAT,
) -> Fallible<Dynamic> {
    if let (Ok(a), Ok(b)) = (a.as_int(), b.as_int()) {
        return Ok(Dynamic::from_int(ints(a, b)));
    }
    Ok(Dynamic::from_float(floats(
        float(a, function)?,
        float(b, function)?,
    )))
}

fn grow(array: &Array) -> Fallible<()> {
    if array.len() >= MAX_ARRAY {
        return Err(runtime(format!(
            "an array holds at most {MAX_ARRAY} entries"
        )));
    }
    Ok(())
}

/// Where `insert` puts a value: a negative index counts from the end, and an
/// index past either end clamps to it.
fn position(len: usize, index: INT) -> usize {
    let len = len as INT;
    let index = if index < 0 { index + len } else { index };
    index.clamp(0, len) as usize
}
