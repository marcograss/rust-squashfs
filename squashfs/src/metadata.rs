use super::{Read, Seek, SqsIoReader, compress};
use byteorder::{ByteOrder, LittleEndian};
use std::io::{Result, SeekFrom};

pub const METADATA_BLOCK_SIZE: usize = 8192;

pub fn read_metadata(
  r: &mut SqsIoReader,
  algorithm: compress::Algorithm,
  first_block: u64,
  block_offset: u32,
  byte_offset: u32,
  size: usize,
) -> Result<Vec<u8>> {
  let mut buf = vec![];
  let mut location = first_block + u64::from(block_offset);

  // read first block
  let (meta, next_block_offset) = read_meta_block(r, algorithm, location as u64)?;
  location = u64::from(next_block_offset);
  buf.extend(&meta[(byte_offset as usize)..]);

  // maybe cross many block, read them all.
  let mut i = 1;
  while buf.len() < size {
    debug!(
      "[read_metadata.read-{}] location={}, buf.size={}\n",
      i,
      location,
      buf.len()
    );
    let (meta, next_block_offset) = read_meta_block(r, algorithm, location as u64)?;
    location = u64::from(next_block_offset);
    buf.extend(meta);
    i += 1;
  }

  Ok(buf)
}

pub fn read_meta_block(
  r: &mut SqsIoReader,
  algorithm: compress::Algorithm,
  location: u64,
) -> Result<(Vec<u8>, u16)> {
  let mut header_bytes = [0u8; 2];

  r.seek(SeekFrom::Start(location))?;
  r.read_exact(&mut header_bytes)?;

  let header = LittleEndian::read_u16(&header_bytes);
  let (size, compressed) = get_metadata_size(header);

  debug!(
    "[read_meta_block] metadata: location={}, header={:?}/{:?} size={}, compressed={}",
    location, header_bytes, header, size, compressed
  );

  let mut buf = vec![0u8; size as usize];
  // Skip header, read data
  r.seek(SeekFrom::Start(location + 2))?;
  r.take(u64::from(size)).read_exact(&mut buf)?;

  trace!(
    "[read_meta_block] raw metadata: data({})={:02x?}",
    buf.len(),
    buf
  );

  let mut output = vec![0u8; METADATA_BLOCK_SIZE];
  if compressed {
    let desize = compress::decompress(&buf, &mut output, algorithm)?;
    let (temp, _) = output.split_at(desize);
    output = temp.to_vec();
  } else {
    return Ok((buf, size + 2));
  }

  trace!(
    "[read_meta_block] decompressed metadata({})={:02x?}",
    output.len(),
    output
  );

  Ok((output, size + 2))
}

/// returns data size and is compresseds
#[must_use] pub fn get_metadata_size(header: u16) -> (u16, bool) {
  let data_size = header & 0x7FFF;
  let compressed = header & 0x8000 != 0x8000;
  (data_size, compressed)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::tests::prepare_tests;
  use std::io::Result;

  /// (header,size,compressed)
  struct TestMetadata(Vec<u8>, u16, bool);

  #[test]
  fn test_get_metadata_size() -> Result<()> {
    let metas: Vec<TestMetadata> = vec![
      TestMetadata([0x25, 0xff].to_vec(), 0x7f25, false),
      TestMetadata([0x25, 0x7f].to_vec(), 0x7f25, true),
    ];

    for TestMetadata(header, should_size, should_compressed) in metas {
      let (size, compressed) = get_metadata_size(LittleEndian::read_u16(&header));
      assert_eq!(size, should_size);
      assert_eq!(compressed, should_compressed);
    }

    Ok(())
  }

  #[test]
  fn test_read_metad_block() -> Result<()> {
    let (mut reader, sb) = prepare_tests()?;
    read_meta_block(&mut reader, sb.compressor, sb.inode_table_start)?;

    Ok(())
  }
}
