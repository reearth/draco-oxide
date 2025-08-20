use std::vec::IntoIter;
use std::mem;

use crate::core::bit_coder::ByteWriter;


const EVAL_BEGIN: u8 = 0xB7;
const EVAL_END: u8 = 0xDC;
const NUM_REPETITIONS: usize = 8;

fn write_eval_begin<W>(writer: &mut W) 
    where W: ByteWriter
{
    for _ in 0..NUM_REPETITIONS {
        writer.write_u8(EVAL_BEGIN);
    }
}

fn write_eval_end<W>(writer: &mut W) 
    where W: ByteWriter
{
    for _ in 0..NUM_REPETITIONS {
        writer.write_u8(EVAL_END);
    }
}

/// writes the given data to the provided writer.
pub fn write_json_pair<W>(key:  &str, val: serde_json::Value, eval_writer: &mut W) 
    where W: ByteWriter
{
    write_eval_begin(eval_writer);
    let data_id = Data::DataValue.to_id();
    eval_writer.write_u8(data_id);
    for data in convert_json_pair_to_writable(key, val) {
        eval_writer.write_u8(data);
    }
    write_eval_end(eval_writer);
}


/// writes the given array element to the provided writer.
/// json_array_scope_begin must be called before this function.
pub fn write_arr_elem<W>(val: serde_json::Value, eval_writer: &mut W) 
    where W: ByteWriter
{
    write_eval_begin(eval_writer);
    let data_id = Data::DataValue.to_id();
    eval_writer.write_u8(data_id);
    for data in convert_json_to_writable(val) {
        eval_writer.write_u8(data);
    }
    write_eval_end(eval_writer);
}


/// begins a new scope for the given key.
pub fn scope_begin<W>(key: &str, eval_writer: &mut W) 
    where W: ByteWriter
{
    write_eval_begin(eval_writer);
    let data_id = Data::BeginScope.to_id();
    eval_writer.write_u8(data_id);
    let state_id = State::ValueWriteInProgress(serde_json::Map::new() /* not used */).get_id();
    eval_writer.write_u8(state_id);
    for w in convert_string_to_writable(key) {
        eval_writer.write_u8(w);
    }
    write_eval_end(eval_writer);
}

/// ends the current scope.
pub fn scope_end<W>(eval_writer: &mut W) 
    where W: ByteWriter
{
    write_eval_begin(eval_writer);
    let data_id = Data::EndScope.to_id();
    eval_writer.write_u8(data_id);
    write_eval_end(eval_writer);
}


/// begins a new scope for the array of the given key.
pub fn array_scope_begin<W>(key: &str, eval_writer: &mut W) 
    where W: ByteWriter
{
    write_eval_begin(eval_writer);
    let data_id = Data::BeginScope.to_id();
    eval_writer.write_u8(data_id);
    let state_id = State::ArrayElementWriteInProgress(Vec::new() /* not used */).get_id();
    eval_writer.write_u8(state_id);
    for w in convert_string_to_writable(key) {
        eval_writer.write_u8(w);
    }
    write_eval_end(eval_writer);
}

/// ends the current array scope.
pub fn array_scope_end<W>(eval_writer: &mut W) 
    where W: ByteWriter
{
    write_eval_begin(eval_writer);
    let data_id = Data::EndScope.to_id();
    eval_writer.write_u8(data_id);
    write_eval_end(eval_writer);
}


/// converts the given key and value to a writable format.
/// this conversion can be reversed by the `convert_writable_to_json` function.
fn convert_json_pair_to_writable(key: &str, val: serde_json::Value) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((key.len() as u64).to_le_bytes());
    out.extend(key.as_bytes());
    out.extend((val.to_string().len() as u64).to_le_bytes());
    out.extend(val.to_string().as_bytes());
    out
}

/// converts the given bytes to a key and a json value.
/// this conversion can be reversed by the `convert_json_to_writable` function.
fn convert_writable_to_json_pair(bytes: &[u8]) -> (String, serde_json::Value) {
    let key_len = bytes[0..8].try_into().unwrap();
    let key_len = u64::from_le_bytes(key_len) as usize;
    let key = {
        let mut key = Vec::with_capacity(key_len);
        for i in 8..key_len+8 {
            key.push(bytes[i] as u8);
        }
        String::from_utf8(key).unwrap()
    };
    let val_len = bytes[key_len + 8..key_len+16].try_into().unwrap();
    let val_len = u64::from_le_bytes(val_len) as usize;
    let val = {
        let mut val = Vec::with_capacity(val_len);
        for i in key_len+16.. key_len+16+val_len{
            val.push(bytes[i] as u8);
        }
        let str = String::from_utf8(val).unwrap();
        serde_json::from_str(&str).unwrap()
    };
    (key, val)
}

/// converts the given json value to a writable format.
/// this conversion can be reversed by the `convert_writable_to_json` function.
fn convert_json_to_writable(val: serde_json::Value) -> Vec<u8> {
    let mut out = Vec::new();
    val.to_string();
    out.extend((val.to_string().len() as u64).to_le_bytes());
    out.extend(val.to_string().as_bytes());
    out
}

/// converts the given bytes to a json object.
/// this conversion can be reversed by the `convert_json_to_writable` function.
fn convert_writable_to_json(bytes: &[u8]) -> serde_json::Value {
    let val_len = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize;
    let val = {
        let mut val = Vec::with_capacity(val_len);
        for i in 8..val_len+8 {
            val.push(bytes[i]);
        }
        let str = String::from_utf8(val).unwrap();
        serde_json::from_str(&str).unwrap()
    };
    val
}


/// converts the given bytes to a string.
fn convert_writable_to_string(bytes: &mut impl Iterator<Item = u8>) -> String {
    let key_len = u64::from_le_bytes(bytes.take(8).collect::<Vec<_>>().try_into().unwrap()) as usize;
    let mut key = Vec::with_capacity(key_len);
    for _ in 0..key_len {
        key.push(bytes.next().unwrap());
    }
    String::from_utf8(key).unwrap()
}

/// converts the given string to a writable format.
/// this conversion can be reversed by the `convert_writable_to_string` function.
fn convert_string_to_writable(key: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((key.len() as u64).to_le_bytes());
    out.extend(key.as_bytes());
    out
}

/// A writer designed to be used to receive data from the encoder and checks if the data is 
/// meant for evaluation or not. If the data is meant for evaluation, it will store it and creates
/// a json object from it. If the data is not meant for evaluation, it will pass it to the provided writer.
pub struct EvalWriter<'a, W> {
    writer: &'a mut W,
    data: Vec<u8>,
    name_state_stack: Vec<(String, State)>,
    data_read_in_progress: bool,
    result: Option<serde_json::Value>,
    data_stack: Vec<u8>
}

impl<'a, W> EvalWriter<'a, W> 
    where W: ByteWriter
{
    pub fn new(writer: &'a mut W) -> Self {
        Self {
            writer,
            data: Vec::new(),
            name_state_stack: Vec::new(),
            data_read_in_progress: false,
            result: None,
            data_stack: Vec::new(),
        }
    }

    fn write_impl(&mut self) {
        if self.data_read_in_progress {
            self.data.extend(mem::take(&mut self.data_stack));
        } else {
            for data in mem::take(&mut self.data_stack) {
                self.writer.write_u8(data);
            }
        }
    }

    pub fn get_result(self) -> serde_json::Value {
        if let Some(result) = self.result {
            return result;
        } else {
            panic!("tryung to get result before it is ready");
        }
    }
}

impl<'a, W> ByteWriter for EvalWriter<'a, W>
    where W: ByteWriter
{
    /// Writes a single byte to the writer. 
    /// If 'EVAL_BEGIN' or 'EVAL_END' is written five times in a row, it will start or end to process
    /// the evaluation data.
    fn write_u8(&mut self, data: u8) {
        if data == EVAL_BEGIN {
            if self.data_stack.iter().all(|&d| d==EVAL_BEGIN) {
                if self.data_stack.len() == NUM_REPETITIONS-1 {
                    if self.data_read_in_progress {
                        panic!("Cannot start a new read while another read is in progress");
                    } else {
                        self.data_read_in_progress = true;
                        self.data_stack.clear();
                    }
                } else {
                    self.data_stack.push(data);
                }
            } else {
                self.write_impl();
                self.data_stack.push(data);
            }
        } else if data == EVAL_END {
            if self.data_stack.iter().all(|&d| d==EVAL_END) {
                if self.data_stack.len() == NUM_REPETITIONS-1 {
                    if self.data_read_in_progress {
                        Data::process(self);
                        self.data_read_in_progress = false;
                        self.data_stack.clear();
                    } else {
                        panic!("Cannot end a read while no read is in progress");
                    }
                } else {
                    self.data_stack.push(data);
                }
            } else {
                self.data_stack.push(data);
            }
        } else {
            self.data_stack.push(data);
            self.write_impl();
        }
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    ValueWriteInProgress(serde_json::Map<String, serde_json::Value> /* json values */),
    ArrayElementWriteInProgress(Vec<serde_json::Value> /* json values */),
}

impl State {
    fn new_from_id(id: u8) -> Self {
        match id {
            0 => State::ValueWriteInProgress(serde_json::Map::new()),
            1 => State::ArrayElementWriteInProgress(Vec::new()),
            _ => panic!("Invalid state id"),
        }
    }

    fn get_id(&self) -> u8 {
        match self {
            State::ValueWriteInProgress(_) => 0,
            State::ArrayElementWriteInProgress(_) => 1,
        }
    }

    fn begin_scope<W>(self, scope_name: String, writer: &mut EvalWriter<W>) {
        writer.name_state_stack.push((scope_name, self));
    }

    fn end_scope<W>(writer: &mut EvalWriter<W>) {
        let (scope_name, state) = writer.name_state_stack.pop().unwrap();
        let parent_state = if let Some((_,parent_state)) = writer.name_state_stack.last_mut() {
            parent_state
        } else {
            // done.
            let result = match state {
                State::ValueWriteInProgress(values) => serde_json::Value::Object(values),
                State::ArrayElementWriteInProgress(values) => serde_json::Value::Array(values),
            };
            let result = serde_json::json!({
                scope_name: result
            });
            writer.result = Some(result);
            return;
        };
        match state {
            State::ValueWriteInProgress(values) => match parent_state {
                State::ValueWriteInProgress(map) => {
                    map.insert(scope_name, serde_json::Value::Object(values));
                }
                State::ArrayElementWriteInProgress(arr) => {
                    arr.push(serde_json::Value::Object(values));
                }
            }
            State::ArrayElementWriteInProgress(values) => match parent_state {
                State::ValueWriteInProgress(map) => {
                    map.insert(scope_name, serde_json::Value::Array(values));
                }
                State::ArrayElementWriteInProgress(arr) => {
                    arr.push(serde_json::Value::Array(values));
                }
            }
        }
    }

    fn process<W>(writer: &mut EvalWriter<W>, data: &mut IntoIter<u8>) {
        let (_,state) = writer.name_state_stack.last_mut().unwrap();
        match state {
            State::ValueWriteInProgress(values) => {
                let data = data.as_slice();
                let (key, val) = convert_writable_to_json_pair(data);
                values.insert(key, val);
            }
            State::ArrayElementWriteInProgress(values) => {
                let data = data.as_slice();
                let val = convert_writable_to_json(data);
                values.push(val);
            }
        }
    }
}


enum Data {
    DataValue,
    BeginScope,
    EndScope,
}

impl Data {
    fn from_id(id: u8) -> Self {
        match id {
            1 => Data::DataValue,
            2 => Data::BeginScope,
            3 => Data::EndScope,
            _ => panic!("Invalid data id"),
        }
    }

    fn to_id(&self) -> u8 {
        match self {
            Data::DataValue => 1,
            Data::BeginScope => 2,
            Data::EndScope => 3,
        }
    }

    fn process<W>(writer: &mut EvalWriter<W>) {
        let mut data = mem::take(&mut writer.data).into_iter();
        let kind = Self::from_id(data.next().unwrap());
        match kind {
            Data::DataValue => {
                State::process(writer, &mut data);
            }
            Data::BeginScope => {
                let state = State::new_from_id(data.next().unwrap());
                let name = convert_writable_to_string(&mut data);
                state.begin_scope(name, writer);
            }
            Data::EndScope => {
                State::end_scope(writer);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::FunctionalByteWriter;

    use super::*;

    #[test]
    fn test_eval_writer() {
        let mut data_written = vec![
            0xA3,
            0xB2,
            0xC1,
        ].into_iter();
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
        assert_eq!(json_data, expected_json,
            "Expected \n{} \nbut got \n{}",
            serde_json::to_string_pretty(&expected_json).unwrap(),
            serde_json::to_string_pretty(&json_data).unwrap()
        );
    }
}