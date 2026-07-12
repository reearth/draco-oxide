//! Evaluation data collection for the analyzer.
//!
//! This module provides functions to collect evaluation/metrics data during encoding.
//! The data is stored in thread-local storage and can be retrieved after encoding.
//! This design ensures that evaluation data never pollutes the actual Draco output.

use std::cell::RefCell;

use crate::core::bit_coder::ByteWriter;

/// Evaluation event types stored in thread-local buffer
#[derive(Debug, Clone)]
enum EvalEvent {
    ScopeBegin { name: String, is_array: bool },
    ScopeEnd,
    Value { key: String, val: serde_json::Value },
    ArrayElement { val: serde_json::Value },
}

thread_local! {
    static EVAL_BUFFER: RefCell<Vec<EvalEvent>> = const { RefCell::new(Vec::new()) };
}

/// Clears the evaluation buffer. Call this before starting a new encode operation.
pub fn clear() {
    EVAL_BUFFER.with(|buf| buf.borrow_mut().clear());
}

/// Takes all evaluation events from the buffer, leaving it empty.
fn take_events() -> Vec<EvalEvent> {
    EVAL_BUFFER.with(|buf| std::mem::take(&mut *buf.borrow_mut()))
}

/// Writes the given data to the evaluation buffer.
/// The writer parameter is kept for API compatibility but is not used.
pub fn write_json_pair<W>(_key: &str, _val: serde_json::Value, _eval_writer: &mut W)
where
    W: ByteWriter,
{
    EVAL_BUFFER.with(|buf| {
        buf.borrow_mut().push(EvalEvent::Value {
            key: _key.to_string(),
            val: _val,
        });
    });
}

/// Writes the given array element to the evaluation buffer.
/// The writer parameter is kept for API compatibility but is not used.
pub fn write_arr_elem<W>(_val: serde_json::Value, _eval_writer: &mut W)
where
    W: ByteWriter,
{
    EVAL_BUFFER.with(|buf| {
        buf.borrow_mut().push(EvalEvent::ArrayElement { val: _val });
    });
}

/// Begins a new scope for the given key.
/// The writer parameter is kept for API compatibility but is not used.
pub fn scope_begin<W>(_key: &str, _eval_writer: &mut W)
where
    W: ByteWriter,
{
    EVAL_BUFFER.with(|buf| {
        buf.borrow_mut().push(EvalEvent::ScopeBegin {
            name: _key.to_string(),
            is_array: false,
        });
    });
}

/// Ends the current scope.
/// The writer parameter is kept for API compatibility but is not used.
pub fn scope_end<W>(_eval_writer: &mut W)
where
    W: ByteWriter,
{
    EVAL_BUFFER.with(|buf| {
        buf.borrow_mut().push(EvalEvent::ScopeEnd);
    });
}

/// Begins a new scope for the array of the given key.
/// The writer parameter is kept for API compatibility but is not used.
pub fn array_scope_begin<W>(_key: &str, _eval_writer: &mut W)
where
    W: ByteWriter,
{
    EVAL_BUFFER.with(|buf| {
        buf.borrow_mut().push(EvalEvent::ScopeBegin {
            name: _key.to_string(),
            is_array: true,
        });
    });
}

/// Ends the current array scope.
/// The writer parameter is kept for API compatibility but is not used.
pub fn array_scope_end<W>(_eval_writer: &mut W)
where
    W: ByteWriter,
{
    EVAL_BUFFER.with(|buf| {
        buf.borrow_mut().push(EvalEvent::ScopeEnd);
    });
}

/// A writer that collects evaluation data from thread-local storage after encoding.
/// Unlike the previous design, this writer simply passes all data through to the
/// underlying writer without modification.
pub struct EvalWriter<'a, W> {
    writer: &'a mut W,
}

impl<'a, W> EvalWriter<'a, W>
where
    W: ByteWriter,
{
    pub fn new(writer: &'a mut W) -> Self {
        // Clear any previous evaluation data
        clear();
        Self { writer }
    }

    /// Gets the evaluation result by processing events from thread-local storage.
    /// This should be called after encoding is complete.
    pub fn get_result(self) -> serde_json::Value {
        let events = take_events();
        process_events(events)
    }
}

impl<W> ByteWriter for EvalWriter<'_, W>
where
    W: ByteWriter,
{
    fn write_u8(&mut self, data: u8) {
        self.writer.write_u8(data);
    }

    fn write_u16(&mut self, value: u16) {
        self.writer.write_u16(value);
    }

    fn write_u24(&mut self, value: u32) {
        self.writer.write_u24(value);
    }

    fn write_u32(&mut self, value: u32) {
        self.writer.write_u32(value);
    }

    fn write_u64(&mut self, value: u64) {
        self.writer.write_u64(value);
    }
}

/// Process evaluation events into a JSON structure
fn process_events(events: Vec<EvalEvent>) -> serde_json::Value {
    #[derive(Debug)]
    enum State {
        Object {
            name: String,
            values: serde_json::Map<String, serde_json::Value>,
        },
        Array {
            name: String,
            values: Vec<serde_json::Value>,
        },
    }

    let mut stack: Vec<State> = Vec::new();

    for event in events {
        match event {
            EvalEvent::ScopeBegin { name, is_array } => {
                if is_array {
                    stack.push(State::Array {
                        name,
                        values: Vec::new(),
                    });
                } else {
                    stack.push(State::Object {
                        name,
                        values: serde_json::Map::new(),
                    });
                }
            }
            EvalEvent::ScopeEnd => {
                let completed = stack.pop().expect("ScopeEnd without matching ScopeBegin");
                let (name, value) = match completed {
                    State::Object { name, values } => (name, serde_json::Value::Object(values)),
                    State::Array { name, values } => (name, serde_json::Value::Array(values)),
                };

                if let Some(parent) = stack.last_mut() {
                    match parent {
                        State::Object { values, .. } => {
                            values.insert(name, value);
                        }
                        State::Array { values, .. } => {
                            values.push(value);
                        }
                    }
                } else {
                    // This is the root scope
                    return serde_json::json!({ name: value });
                }
            }
            EvalEvent::Value { key, val } => {
                if let Some(State::Object { values, .. }) = stack.last_mut() {
                    values.insert(key, val);
                }
            }
            EvalEvent::ArrayElement { val } => {
                if let Some(State::Array { values, .. }) = stack.last_mut() {
                    values.push(val);
                }
            }
        }
    }

    // If we get here without returning, return empty object
    serde_json::Value::Object(serde_json::Map::new())
}

#[cfg(test)]
mod tests {
    use draco_oxide_core::bit_coder::FunctionalByteWriter;

    use super::*;

    #[test]
    fn test_eval_writer() {
        let mut data_written = vec![0xA3, 0xB2, 0xC1].into_iter();
        let mut writer = |input| {
            assert_eq!(input, data_written.next().unwrap());
        };

        let mut writer = FunctionalByteWriter::new(&mut writer);
        let mut writer = EvalWriter::new(&mut writer);

        scope_begin("family", &mut writer);
        write_json_pair("number", 2.into(), &mut writer);
        array_scope_begin("people", &mut writer);

        scope_begin("person1", &mut writer);
        write_json_pair("name", "Alice".into(), &mut writer);
        writer.write_u8(0xA3);
        write_json_pair("age", 20.into(), &mut writer);
        scope_end(&mut writer);

        scope_begin("person2", &mut writer);
        writer.write_u8(0xB2);
        write_json_pair("name", "Bob".into(), &mut writer);
        write_json_pair("age", 21.into(), &mut writer);
        scope_end(&mut writer);

        array_scope_end(&mut writer);

        array_scope_begin("assets", &mut writer);
        write_arr_elem("house".into(), &mut writer);
        write_arr_elem("car".into(), &mut writer);
        array_scope_end(&mut writer);

        scope_end(&mut writer);
        writer.write_u8(0xC1);

        let json_data = writer.get_result();
        let expected_json = serde_json::json!({
            "family": {
                "number": 2,
                "people": [
                    {
                        "name": "Alice",
                        "age": 20
                    },
                    {
                        "name": "Bob",
                        "age": 21
                    }
                ],
                "assets": [
                    "house",
                    "car"
                ]
            }
        });
        assert_eq!(
            json_data,
            expected_json,
            "Expected \n{} \nbut got \n{}",
            serde_json::to_string_pretty(&expected_json).unwrap(),
            serde_json::to_string_pretty(&json_data).unwrap()
        );
    }

    #[test]
    fn test_no_pollution() {
        // Test that evaluation data doesn't pollute the output buffer
        let mut output = Vec::new();
        {
            let mut writer = EvalWriter::new(&mut output);
            scope_begin("test", &mut writer);
            writer.write_u8(0xAB);
            writer.write_u8(0xCD);
            write_json_pair("key", "value".into(), &mut writer);
            scope_end(&mut writer);
            writer.write_u8(0xEF);
            let _ = writer.get_result();
        }
        // Output should only contain the actual data, not evaluation markers
        assert_eq!(output, vec![0xAB, 0xCD, 0xEF]);
    }

    #[test]
    fn test_clear() {
        // Test that clear() works
        scope_begin::<Vec<u8>>("test", &mut vec![]);
        assert!(!take_events().is_empty()); // Events were added
        clear();
        assert!(take_events().is_empty());
    }
}
