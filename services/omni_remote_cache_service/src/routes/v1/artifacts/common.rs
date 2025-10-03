pub fn key(digest: &str, ws: &str, env: &str) -> String {
    let ws = bs58::encode(ws.as_bytes()).into_string();
    let env = bs58::encode(env.as_bytes()).into_string();
    let digest = bs58::encode(digest.as_bytes()).into_string();
    format!("{}-{}-{}", ws, env, digest)
}
