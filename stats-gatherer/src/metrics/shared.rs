use serde_repr::{
    Deserialize_repr,
    Serialize_repr,
};

#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MediaType {
    Audio = 0,
    Video = 1,
}

#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Direction {
    Tx = 0,
    Rx = 1,
}

#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Unit {
    Packets = 0,
    BitsPerSecond = 1,
    BytesPerSecond = 2,
    Microseconds = 3,
    Percent = 4,
    Count = 5,
}

#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MetricType {
    Gauge = 0,
    Counter = 1,
    Histogram = 2,
}

#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum StreamType {
    Audio = 0,
    Video = 1,
}

#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ParticipantType {
    Sender = 0,
    Receiver = 1,
}

impl MediaType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MediaType::Audio => "audio",
            MediaType::Video => "video",
        }
    }
}

impl Direction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Direction::Tx => "tx",
            Direction::Rx => "rx",
        }
    }
}

impl Unit {
    pub fn as_str(&self) -> &'static str {
        match self {
            Unit::Packets => "packets",
            Unit::BitsPerSecond => "bps",
            Unit::BytesPerSecond => "Bps",
            Unit::Microseconds => "μs",
            Unit::Percent => "%",
            Unit::Count => "count",
        }
    }
}

impl StreamType {
    pub fn as_str(&self) -> &'static str {
        match self {
            StreamType::Audio => "audio",
            StreamType::Video => "video",
        }
    }
}

impl ParticipantType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParticipantType::Sender => "sender",
            ParticipantType::Receiver => "receiver",
        }
    }
}

impl MetricType {
    pub fn metric_type_str(&self) -> &'static str {
        match self {
            MetricType::Gauge => "gauge",
            MetricType::Counter => "counter",
            MetricType::Histogram => "histogram",
        }
    }
}
