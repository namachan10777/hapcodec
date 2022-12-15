use std::io::{self, Read};

use byteorder::ReadBytesExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    RGB,
    RGBA,
    ScaledYCoCg,
    Alpha,
    RGBUnsignedFloat,
    RGBSignedFloat,
    MultipleImages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelCompression {
    DXT1BC1,
    DXT5BC3,
    BC7,
    RGTC1BC4,
    BC6U,
    BC6S,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondStageCompressor {
    None,
    Snappy,
    Complex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub section_size: u32,
    pub pixel_format: PixelFormat,
    pub pixel_compression: PixelCompression,
    pub second_stage_compressor: SecondStageCompressor,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown compressor")]
    UnknownCompressor(u8),
    #[error("unknown texture format")]
    UnknownTextureFormat(u8),
    #[error("IO error {0}")]
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

struct RawSection {
    size: u32,
    section_type: u8,
}

fn parse_section_header<R: Read>(r: &mut R) -> io::Result<RawSection> {
    let section_size = r.read_u24::<byteorder::LE>()?;
    let section_type = r.read_u8()?;
    let section_size = if section_size == 0 {
        r.read_u32::<byteorder::LE>()?
    } else {
        section_size
    };
    Ok(RawSection {
        size: section_size,
        section_type,
    })
}

pub fn parse_toplevel_section<R: Read>(r: &mut R) -> Result<Header, Error> {
    let raw_section = parse_section_header(r)?;
    let (pixel_format, pixel_compression) = match raw_section.section_type & 0x0F {
        0x0B => (PixelFormat::RGB, PixelCompression::DXT1BC1),
        0x0E => (PixelFormat::RGBA, PixelCompression::DXT5BC3),
        0x0F => (PixelFormat::ScaledYCoCg, PixelCompression::DXT5BC3),
        0x0C => (PixelFormat::RGBA, PixelCompression::BC7),
        0x01 => (PixelFormat::Alpha, PixelCompression::RGTC1BC4),
        0x02 => (PixelFormat::RGBUnsignedFloat, PixelCompression::BC6U),
        0x03 => (PixelFormat::RGBSignedFloat, PixelCompression::BC6S),
        0x0D => (PixelFormat::MultipleImages, PixelCompression::NotApplicable),
        unknown => return Err(Error::UnknownTextureFormat(unknown)),
    };
    let second_stage_compressor = match raw_section.section_type & 0xF0 {
        0xA0 => SecondStageCompressor::None,
        0xB0 => SecondStageCompressor::Snappy,
        0xC0 => SecondStageCompressor::Complex,
        unknown => return Err(Error::UnknownCompressor(unknown)),
    };

    Ok(Header {
        second_stage_compressor,
        section_size: raw_section.size,
        pixel_compression,
        pixel_format,
    })
}
