use std::io::{self, Read};

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
    RGB_DXT1_BC1(RawTexture),
    RGBA_DXT5_BC3(RawTexture),
    ScaledYCoCg_DXT5_BC3(RawTexture),
    RGBA_BC7(RawTexture),
    Alpha_RGTC1_BC4(RawTexture),
    RGBUnsignedFloat_BC6U(RawTexture),
    RGBSignedFloat_BC6S(RawTexture),
    MultipleImages_ScaledYCoCg_DXT5_Alpha_RGTC1(RawTexture, RawTexture),
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
