use std::{
    fmt::Debug,
    io::{self, Read},
};

use byteorder::{ReadBytesExt, LE};

const HAP_SECITON_CHUNK_SECOND_STAGE_COMPRESSOR_TABLE: u8 = 0x02;
const HAP_SECTION_CHUNK_SIZE_TABLE: u8 = 0x03;
const HAP_SECTION_CHUNK_OFFSET_TABLE: u8 = 0x04;

pub type RawTexture = Vec<u8>;

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

#[allow(non_camel_case_types)]
pub enum Texture {
    /// GL_COMPRESSED_RGB_S3TC_DXT1_EXT
    RGB_DXT1_BC1(RawTexture),
    /// GL_COMPRESSED_RGBA_S3TC_DXT5_EXT
    RGBA_DXT5_BC3(RawTexture),
    ScaledYCoCg_DXT5_BC3(RawTexture),
    /// GL_COMPRESSED_RGBA_BPTC_UNORM_ARB
    RGBA_BC7(RawTexture),
    Alpha_RGTC1_BC4(RawTexture),
    /// GL_COMPRESSED_RGB_BPTC_UNSIGNED_FLOAT_ARB
    RGBUnsignedFloat_BC6U(RawTexture),
    /// GL_COMPRESSED_RGB_BPTC_SIGNED_FLOAT_ARB
    RGBSignedFloat_BC6S(RawTexture),
    MultipleImages_ScaledYCoCg_DXT5_Alpha_RGTC1(RawTexture, RawTexture),
}

#[cfg(feature = "opengl")]
pub enum OpenGLFormatId {
    Single(gl::types::GLenum),
    Double(gl::types::GLenum, gl::types::GLenum),
    Unsupported,
}

impl Debug for Texture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Texture");
        match self {
            Self::RGB_DXT1_BC1(inner) => s
                .field("color", &"RGB")
                .field("compression", &"DXT1/BC1")
                .field("size", &inner.len())
                .finish(),
            Self::RGBA_DXT5_BC3(inner) => s
                .field("color", &"RGBA")
                .field("compression", &"DXT5/BC3")
                .field("size", &inner.len())
                .finish(),
            Self::ScaledYCoCg_DXT5_BC3(inner) => s
                .field("color", &"ScaledYCoCg")
                .field("compression", &"DXT5/BC3")
                .field("size", &inner.len())
                .finish(),
            Self::RGBA_BC7(inner) => s
                .field("color", &"RGBA")
                .field("compression", &"BC7")
                .field("size", &inner.len())
                .finish(),
            Self::Alpha_RGTC1_BC4(inner) => s
                .field("color", &"Alpha")
                .field("compression", &"RGTC1/BC4")
                .field("size", &inner.len())
                .finish(),
            Self::RGBUnsignedFloat_BC6U(inner) => s
                .field("color", &"RGB unsigned float")
                .field("compression", &"BC6U")
                .field("size", &inner.len())
                .finish(),
            Self::RGBSignedFloat_BC6S(inner) => s
                .field("color", &"RGB signed float")
                .field("compression", &"BC6S")
                .field("size", &inner.len())
                .finish(),
            Self::MultipleImages_ScaledYCoCg_DXT5_Alpha_RGTC1(inner1, inner2) => s
                .field("color1", &"ScaledYCoCg")
                .field("color2", &"Alpha")
                .field("compression1", &"DXT5/BC3")
                .field("compression2", &"BC4")
                .field("size1", &inner1.len())
                .field("size2", &inner2.len())
                .finish(),
        }
    }
}

impl Texture {
    #[cfg(feature = "opengl")]
    pub fn opengl_pixelformat_id(&self) -> OpenGLFormatId {
        match self {
            Self::RGB_DXT1_BC1(_) => OpenGLFormatId::Single(0x83F0),
            Self::RGBA_DXT5_BC3(_) => OpenGLFormatId::Single(0x83F3),
            Self::ScaledYCoCg_DXT5_BC3(_) => OpenGLFormatId::Unsupported,
            Self::RGBA_BC7(_) => OpenGLFormatId::Single(0x8E8C),
            Self::Alpha_RGTC1_BC4(_) => OpenGLFormatId::Unsupported,
            Self::RGBUnsignedFloat_BC6U(_) => OpenGLFormatId::Single(0x8E8F),
            Self::RGBSignedFloat_BC6S(_) => OpenGLFormatId::Single(0x8E8E),
            Self::MultipleImages_ScaledYCoCg_DXT5_Alpha_RGTC1(_, _) => OpenGLFormatId::Unsupported,
        }
    }
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
    #[error("unknown decode instruction")]
    UnknownDecodeInstruction(u8),
    #[error("failed to decompress due to {0}")]
    Snappy(snap::Error),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

struct RawSection {
    size: u32,
    section_type: u8,
    header_size: usize,
}

fn parse_section_header<R: Read>(r: &mut R) -> io::Result<RawSection> {
    let section_size = r.read_u24::<byteorder::LE>()?;
    let section_type = r.read_u8()?;
    let (section_size, header_size) = if section_size == 0 {
        (r.read_u32::<byteorder::LE>()?, 4 + 4)
    } else {
        (section_size, 4)
    };
    Ok(RawSection {
        size: section_size,
        section_type,
        header_size,
    })
}

fn decode_second_stage_compressor(compressor: u8) -> Result<SecondStageCompressor, Error> {
    match compressor & 0xF0 {
        0xA0 => Ok(SecondStageCompressor::None),
        0xB0 => Ok(SecondStageCompressor::Snappy),
        unknown => Err(Error::UnknownCompressor(unknown)),
    }
}

struct ChunkInfo {
    offset: usize,
    size: usize,
    compressor: SecondStageCompressor,
}

fn decode_complex_instruction<R: Read>(r: &mut R) -> Result<(usize, Vec<ChunkInfo>), Error> {
    let complex_header = parse_section_header(r)?;
    let mut bytestreaming = complex_header.size as usize;
    let mut compressors = Vec::new();
    let mut chunk_sizes = Vec::new();
    let mut chunk_offsets = Vec::new();
    while bytestreaming != 0 {
        let instruction_header = parse_section_header(r)?;
        bytestreaming -= instruction_header.header_size + instruction_header.size as usize;
        let mut buf = Vec::new();
        buf.resize(instruction_header.size as usize, 0);
        r.read_exact(&mut buf)?;
        match instruction_header.section_type {
            HAP_SECITON_CHUNK_SECOND_STAGE_COMPRESSOR_TABLE => {
                compressors = buf
                    .into_iter()
                    .map(decode_second_stage_compressor)
                    .collect::<Result<Vec<_>, _>>()?;
            }
            HAP_SECTION_CHUNK_OFFSET_TABLE => {
                for mut chunk_offset in buf.chunks(4) {
                    chunk_offsets.push(chunk_offset.read_u32::<LE>()?);
                }
            }
            HAP_SECTION_CHUNK_SIZE_TABLE => {
                for mut chunk_size in buf.chunks(4) {
                    chunk_sizes.push(chunk_size.read_u32::<LE>()?);
                }
            }
            _ => (),
        };
    }
    let mut chunks = Vec::new();
    let mut size_subtotal = 0;
    for chunk_idx in 0..chunk_sizes.len() {
        let offset = if chunk_offsets.is_empty() {
            size_subtotal
        } else {
            chunk_offsets[chunk_idx]
        } as usize;
        size_subtotal += chunk_sizes[chunk_idx];
        chunks.push(ChunkInfo {
            size: chunk_sizes[chunk_idx] as usize,
            compressor: compressors[chunk_idx],
            offset,
        });
    }
    Ok((
        complex_header.header_size + complex_header.size as usize,
        chunks,
    ))
}

fn decode_texture<R: Read>(raw_section: RawSection, r: &mut R) -> Result<(RawTexture, u8), Error> {
    let mut decoded_raw_data = Vec::new();
    if raw_section.section_type & 0xF0 == 0xC0 {
        let (consumed_size, chunk_infos) = decode_complex_instruction(r)?;
        let mut buf = Vec::new();
        buf.resize(raw_section.size as usize - consumed_size, 0);
        r.read_exact(&mut buf)?;
        for chunk_info in chunk_infos {
            let mut decoder = snap::raw::Decoder::new();
            if chunk_info.compressor == SecondStageCompressor::Snappy {
                decoded_raw_data.append(
                    &mut decoder
                        .decompress_vec(
                            &buf[chunk_info.offset..chunk_info.offset + chunk_info.size],
                        )
                        .map_err(Error::Snappy)?,
                );
            } else {
                decoded_raw_data.append(&mut buf);
            }
        }
    } else if raw_section.section_type & 0xF0 == 0xA0 {
        decoded_raw_data.resize(raw_section.size as usize, 0);
        r.read_exact(&mut decoded_raw_data)?;
    } else if raw_section.section_type & 0xF0 == 0xB0 {
        let mut buf = Vec::new();
        buf.resize(raw_section.size as usize, 0);
        r.read_exact(&mut buf)?;
        let mut decoder = snap::raw::Decoder::new();
        decoded_raw_data = decoder.decompress_vec(&buf).map_err(Error::Snappy)?;
    } else {
        return Err(Error::UnknownCompressor(raw_section.section_type & 0xF0));
    }
    Ok((decoded_raw_data, raw_section.section_type & 0x0F))
}

fn wrap_single_texture(texture_format: u8, raw: RawTexture) -> Result<Texture, Error> {
    Ok(match texture_format & 0x0f {
        0x0b => Texture::RGB_DXT1_BC1(raw),
        0x0e => Texture::RGBA_DXT5_BC3(raw),
        0x0f => Texture::ScaledYCoCg_DXT5_BC3(raw),
        0x0c => Texture::RGBA_BC7(raw),
        0x01 => Texture::Alpha_RGTC1_BC4(raw),
        0x02 => Texture::RGBUnsignedFloat_BC6U(raw),
        0x03 => Texture::RGBSignedFloat_BC6S(raw),
        _ => return Err(Error::UnknownTextureFormat(texture_format & 0x0f)),
    })
}

pub fn decode_frame<R: Read>(r: &mut R) -> Result<Texture, Error> {
    let raw_section = parse_section_header(r)?;
    if raw_section.section_type == 0x0d {
        let texture_section_header = parse_section_header(r)?;
        if texture_section_header.header_size + texture_section_header.size as usize
            == raw_section.size as usize
        {
            let (raw, texture_format) = decode_texture(texture_section_header, r)?;
            wrap_single_texture(texture_format, raw)
        } else {
            let (dxt5, _) = decode_texture(texture_section_header, r)?;
            let texture_section_header = parse_section_header(r)?;
            let (rgtc1, _) = decode_texture(texture_section_header, r)?;
            Ok(Texture::MultipleImages_ScaledYCoCg_DXT5_Alpha_RGTC1(
                dxt5, rgtc1,
            ))
        }
    } else {
        let (raw, texture_format) = decode_texture(raw_section, r)?;
        wrap_single_texture(texture_format, raw)
    }
}
