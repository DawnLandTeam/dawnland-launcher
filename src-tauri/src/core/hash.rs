use sha1::Sha1;
use sha2::Sha512;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

/// Calculate SHA-1 and SHA-512 hashes for a file in a shared sequential pass
pub async fn calculate_file_hashes<P: AsRef<Path>>(
    path: P,
) -> std::io::Result<(String, String)> {
    use sha1::Digest as Sha1Digest;
    use sha2::Digest as Sha512Digest;

    let mut file = File::open(path).await?;
    let mut buffer = [0; 8192];
    
    let mut hasher_sha1 = Sha1::new();
    let mut hasher_sha512 = Sha512::new();

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher_sha1.update(&buffer[..n]);
        hasher_sha512.update(&buffer[..n]);
    }

    let result_sha1 = hasher_sha1.finalize();
    let result_sha512 = hasher_sha512.finalize();

    Ok((
        hex::encode(result_sha1),
        hex::encode(result_sha512),
    ))
}

/// Compute CurseForge's Murmur2 hash for a file.
/// CurseForge ignores certain whitespace characters before hashing.
pub async fn calculate_curseforge_hash<P: AsRef<Path>>(path: P) -> std::io::Result<u32> {
    let mut file = File::open(path).await?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await?;
    
    let mut normalized = Vec::with_capacity(buffer.len());
    for &b in &buffer {
        if b == 9 || b == 10 || b == 13 || b == 32 {
            continue;
        }
        normalized.push(b);
    }
    
    Ok(murmur2(&normalized))
}

fn murmur2(data: &[u8]) -> u32 {
    let m: u32 = 0x5bd1e995;
    let r = 24;
    let mut h = 1 ^ (data.len() as u32);
    
    let (chunks, rem) = data.as_chunks::<4>();
    for chunk in chunks {
        let mut k = u32::from_le_bytes(*chunk);
        k = k.wrapping_mul(m);
        k ^= k >> r;
        k = k.wrapping_mul(m);
        h = h.wrapping_mul(m);
        h ^= k;
    }
    match rem.len() {
        3 => {
            h ^= (rem[2] as u32) << 16;
            h ^= (rem[1] as u32) << 8;
            h ^= rem[0] as u32;
            h = h.wrapping_mul(m);
        }
        2 => {
            h ^= (rem[1] as u32) << 8;
            h ^= rem[0] as u32;
            h = h.wrapping_mul(m);
        }
        1 => {
            h ^= rem[0] as u32;
            h = h.wrapping_mul(m);
        }
        _ => {}
    }
    
    h ^= h >> 13;
    h = h.wrapping_mul(m);
    h ^= h >> 15;
    
    h
}
