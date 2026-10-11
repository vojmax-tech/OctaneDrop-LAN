use base64::engine::general_purpose::URL_SAFE;
use base64::Engine;
use rand::RngCore;
use scrypt::{scrypt, Params};
use subtle::ConstantTimeEq;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    if password.is_empty() {
        anyhow::bail!("Password cannot be empty.");
    }
    let n: u32 = 2_u32.pow(14);
    let r: u32 = 8;
    let p: u32 = 1;

    let mut salt = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let params = Params::new(14, r, p, 32)?;
    let mut derived = [0_u8; 32];
    scrypt(password.as_bytes(), &salt, &params, &mut derived)?;

    Ok(format!(
        "scrypt${}${}${}${}${}",
        n,
        r,
        p,
        URL_SAFE.encode(salt),
        URL_SAFE.encode(derived)
    ))
}

pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    let parts: Vec<&str> = stored_hash.split('$').collect();
    if parts.len() != 6 || parts[0] != "scrypt" {
        return false;
    }
    let Ok(n) = parts[1].parse::<u32>() else {
        return false;
    };
    let Ok(r) = parts[2].parse::<u32>() else {
        return false;
    };
    let Ok(p) = parts[3].parse::<u32>() else {
        return false;
    };
    let Ok(salt) = URL_SAFE.decode(parts[4].as_bytes()) else {
        return false;
    };
    let Ok(expected) = URL_SAFE.decode(parts[5].as_bytes()) else {
        return false;
    };
    let log_n = match n.checked_ilog2() {
        Some(value) => value as u8,
        None => return false,
    };
    let Ok(params) = Params::new(log_n, r, p, expected.len()) else {
        return false;
    };
    let mut actual = vec![0_u8; expected.len()];
    if scrypt(password.as_bytes(), &salt, &params, &mut actual).is_err() {
        return false;
    }
    actual.ct_eq(&expected).into()
}

