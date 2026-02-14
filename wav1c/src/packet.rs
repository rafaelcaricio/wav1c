#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameType {
    Key,
    Inter,
}

#[derive(Debug)]
pub struct Packet {
    pub data: Vec<u8>,
    pub frame_type: FrameType,
    pub frame_number: u64,
}
