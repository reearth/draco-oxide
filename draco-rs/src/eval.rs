use std::vec::IntoIter;
use std::{mem, vec};

use serde::Serialize;

use crate::core::attribute::{AttributeId, ComponentDataType}; 
use crate::prelude::{Attribute, AttributeType};

#[derive(Debug, Clone, Serialize)]
pub struct CompressionInfo {
    pub connectivity: ConnectivityInfo,
    pub attributes: Vec<AttributeInfo>,
    pub original_size: usize,
    pub compressed_size: usize,
    pub time_taken: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectivityInfo {
    pub num_faces: usize,
    pub num_vertices: usize,
    pub connectivirty_encoder: ConnectivityEncoder,
    pub compressed_size: usize,
    pub time_taken: f32,
}

#[derive(Debug, Clone, Serialize)]
pub enum ConnectivityEncoder {
    Sequential(Sequential),
    Edgebreaker(Edgebreaker),
}

#[derive(Debug, Clone, Serialize)]
pub struct Sequential {
    pub index_size: usize,
    pub faces: Vec<[usize; 3]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Edgebreaker {
    pub clers_string: Vec<char>
}

#[derive(Debug, Clone, Serialize)]
pub struct AttributeInfo {
    pub time_taken: f32,
    pub attribute_type: AttributeType,
    
    /// The size of the original data in bytes.
    /// This is the size of the attribute values, but not the size of the attribute itself, that is,
    /// the size of the other fields in the `Attribute` struct (such as the size of the
    /// attribute type) is not included.
    pub original_size: usize,
    pub compressed_size: usize,
    pub component_type: ComponentDataType,
    pub num_components: usize,
    pub num_parents: usize,
    pub parents: Vec<AttributeId>,
    pub attribute: Attribute,
}


const EVAL_BEGIN: u64 = 0xDB1EFDF1E7DB5EAC;
const EVAL_END: u64 = 0xECDA5FA8DB86FDE9;

/// writes the given data to the provided writer.
pub fn write_json_pair<F>(key:  &str, val: serde_json::Value, writer: &mut F) 
    where F: FnMut((u8, u64)) 
{
    writer((1, EVAL_BEGIN));
    let data_id = Data::DataValue.to_id();
    writer((8, data_id as u64));
    for data in convert_json_pair_to_writable(key, val) {
        writer(data);
    }
    writer((1, EVAL_END));
}


/// writes the given array element to the provided writer.
/// json_array_scope_begin must be called before this function.
pub fn write_arr_elem<F>(val: serde_json::Value, writer: &mut F) 
    where F: FnMut((u8, u64)) 
{
    writer((1, EVAL_BEGIN));
    let data_id = Data::DataValue.to_id();
    writer((8, data_id as u64));
    for data in convert_json_to_writable(val) {
        writer(data);
    }
    writer((1, EVAL_END));
}


/// begins a new scope for the given key.
pub fn scope_begin<F>(key: &str, writer: &mut F) 
    where F: FnMut((u8, u64)) 
{
    writer((1, EVAL_BEGIN));
    let data_id = Data::BeginScope.to_id();
    writer((8, data_id as u64));
    let state_id = State::ValueWriteInProgress(serde_json::Map::new() /* not used */).get_id();
    writer((8, state_id as u64));
    for w in convert_string_to_writable(key) {
        writer(w);
    }
    writer((1, EVAL_END));
}

/// ends the current scope.
pub fn scope_end<F>(writer: &mut F) 
    where F: FnMut((u8, u64)) 
{
    writer((1, EVAL_BEGIN));
    let data_id = Data::EndScope.to_id();
    writer((8, data_id as u64));
    writer((1, EVAL_END));
}


/// begins a new scope for the array of the given key.
pub fn array_scope_begin<F>(key: &str, writer: &mut F) 
    where F: FnMut((u8, u64)) 
{
    writer((1, EVAL_BEGIN));
    let data_id = Data::BeginScope.to_id();
    writer((8, data_id as u64));
    let state_id = State::ArrayElementWriteInProgress(Vec::new() /* not used */).get_id();
    writer((8, state_id as u64));
    for w in convert_string_to_writable(key) {
        writer(w);
    }
    writer((1, EVAL_END));
}

/// ends the current array scope.
pub fn array_scope_end<F>(writer: &mut F) 
    where F: FnMut((u8, u64)) 
{
    writer((1, EVAL_BEGIN));
    let data_id = Data::EndScope.to_id();
    writer((8, data_id as u64));
    writer((1, EVAL_END));
}


/// converts the given key and value to a writable format.
/// this conversion can be reversed by the `convert_writable_to_json` function.
fn convert_json_pair_to_writable(key: &str, val: serde_json::Value) -> Vec<(u8, u64)> {
    let mut out = vec![(64, key.len() as u64)];
    out.extend(key.as_bytes().iter().map(|&b| (8, b as u64)));
    out.push((64, val.to_string().len() as u64));
    out.extend(val.to_string().as_bytes().iter().map(|&b| (8, b as u64)));
    out
}

/// converts the given bytes to a key and a json value.
/// this conversion can be reversed by the `convert_json_to_writable` function.
fn convert_writable_to_json_pair(bytes: &[(u8, u64)]) -> (String, serde_json::Value) {
    let key_len = bytes[0].1 as usize;
    let key = {
        let mut key = Vec::with_capacity(key_len);
        for i in 1..=key_len {
            key.push(bytes[i].1 as u8);
        }
        String::from_utf8(key).unwrap()
    };
    let val_len = bytes[key_len + 1].1 as usize;
    let val = {
        let mut val = Vec::with_capacity(val_len);
        for i in key_len + 2..=key_len + 1 + val_len {
            val.push(bytes[i].1 as u8);
        }
        let str = String::from_utf8(val).unwrap();
        serde_json::from_str(&str).unwrap()
    };
    (key, val)
}

/// converts the given json value to a writable format.
/// this conversion can be reversed by the `convert_writable_to_json` function.
fn convert_json_to_writable(val: serde_json::Value) -> Vec<(u8, u64)> {
    let mut out = vec![(64, val.to_string().len() as u64)];
    out.extend(val.to_string().as_bytes().iter().map(|&b| (8, b as u64)));
    out
}

/// converts the given bytes to a json object.
/// this conversion can be reversed by the `convert_json_to_writable` function.
fn convert_writable_to_json(bytes: &[(u8, u64)]) -> serde_json::Value {
    let val_len = bytes[0].1 as usize;
    let val = {
        let mut val = Vec::with_capacity(val_len);
        for i in 1..=val_len {
            val.push(bytes[i].1 as u8);
        }
        let str = String::from_utf8(val).unwrap();
        serde_json::from_str(&str).unwrap()
    };
    val
}


/// converts the given bytes to a string.
fn convert_writable_to_string(bytes: &mut impl Iterator<Item = (u8, u64)>) -> String {
    let key_len = bytes.next().unwrap().1 as usize;
    let mut key = Vec::with_capacity(key_len);
    for _ in 0..key_len {
        let (size, val) = bytes.next().unwrap();
        assert!(size == 8, "Invalid size for key");
        key.push(val as u8);
    }
    String::from_utf8(key).unwrap()
}

/// converts the given string to a writable format.
/// this conversion can be reversed by the `convert_writable_to_string` function.
fn convert_string_to_writable(key: &str) -> Vec<(u8, u64)> {
    let mut out = vec![(64, key.len() as u64)];
    out.extend(key.as_bytes().iter().map(|&b| (8, b as u64)));
    out
}

/// A writer designed to be used to receive data from the encoder and checks if the data is 
/// meant for evaluation or not. If the data is meant for evaluation, it will store it and creates
/// a json object from it. If the data is not meant for evaluation, it will pass it to the provided writer.
pub struct EvalWriter<'a, F> {
    writer: &'a mut F,
    data: Vec<(u8, u64)>,
    name_state_stack: Vec<(String, State)>,
    data_read_in_progress: bool,
    result: Option<serde_json::Value>,
}

impl<'a, F> EvalWriter<'a, F> 
    where F: FnMut((u8, u64))
{
    pub fn new(writer: &'a mut F) -> Self {
        Self {
            writer,
            data: Vec::new(),
            name_state_stack: Vec::new(),
            data_read_in_progress: false,
            result: None,
        }
    }

    pub fn write(&mut self, data: (u8, u64)) {
        if self.data_read_in_progress {
            assert!(
                data != Self::read_begin_command(),
                "Cannot start a new read while another read is in progress"
            );
            if data == Self::read_end_command() {
                self.data_read_in_progress = false;
                Data::process(self);
                assert!(self.data.len() == 0, "Data must be consumed at this point.");
            } else {
                self.data.push(data);
            }
        } else { // if data read is not in progress
            assert!(
                data != Self::read_end_command(),
                "Cannot end a read because no read is in progress"
            );
            if data == Self::read_begin_command() {
                self.data_read_in_progress = true;
            } else {
                (self.writer)(data);
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

    fn read_begin_command() -> (u8, u64) {
        (1, EVAL_BEGIN)
    }

    fn read_end_command() -> (u8, u64) {
        (1, EVAL_END)
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

    fn begin_scope<F>(self, scope_name: String, writer: &mut EvalWriter<F>) {
        writer.name_state_stack.push((scope_name, self));
    }

    fn end_scope<F>(writer: &mut EvalWriter<F>) {
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

    fn process<F>(writer: &mut EvalWriter<F>, data: &mut IntoIter<(u8, u64)>) {
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

    fn process<F>(writer: &mut EvalWriter<F>) {
        let mut data = mem::take(&mut writer.data).into_iter();
        let kind = Self::from_id(data.next().unwrap().1 as u8);
        match kind {
            Data::DataValue => {
                State::process(writer, &mut data);
            }
            Data::BeginScope => {
                let state = State::new_from_id(data.next().unwrap().1 as u8);
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
    use super::*;

    #[test]
    fn test_eval_writer() {
        let mut data_written = vec![
            ( 43, 0x12345678),
            ( 67, 0x12345678),
            (163, 0x12345678),
        ].into_iter();
        let mut writer = |input| {
            assert_eq!(input, data_written.next().unwrap());
        };

        let mut eval_writer = EvalWriter::new(&mut writer);
        let mut writer = |input| eval_writer.write(input);

        scope_begin("family", &mut writer);
        write_json_pair("number", 2.into(), &mut writer);
        array_scope_begin("people", &mut writer);
        
        scope_begin("person1", &mut writer);
        write_json_pair("name", "Alice".into(), &mut writer); writer((43, 0x12345678));
        write_json_pair("age", 20.into(), &mut writer);
        scope_end(&mut writer);

        scope_begin("person2", &mut writer); writer((67, 0x12345678));
        write_json_pair("name", "Bob".into(), &mut writer);
        write_json_pair("age", 21.into(), &mut writer);
        scope_end(&mut writer);

        array_scope_end(&mut writer);

        array_scope_begin("assets", &mut writer);
        write_arr_elem("house".into(), &mut writer);
        write_arr_elem("car".into(), &mut writer);
        array_scope_end(&mut writer);
        
        scope_end(&mut writer); writer((163, 0x12345678));

        let json_data = eval_writer.get_result();
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