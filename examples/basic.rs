use std::error::Error;
use std::fs;
use std::path::Path;

use walrus::Walrus;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let db_path = Path::new("test.db");
    if db_path.exists() {
        fs::remove_file(db_path)?;
    }

    // 1. sync engine writes
    {
        let mut db = Walrus::setup(db_path)?;

        db.put(b"user:1001", b"alice")?;
        db.put(b"user:1002", b"bob")?;

        let val = db.get(b"user:1001")?;
        println!("got user:1001: {}", String::from_utf8_lossy(&val));

        db.del(b"user:1002")?;
        assert!(db.get(b"user:1002").is_err());

        db.compact()?;
    }

    // 2. verify recovery from disk
    {
        let mut recovered = Walrus::setup(db_path)?;
        let val = recovered.get(b"user:1001")?;
        assert_eq!(&val[..], b"alice");
    }

    // 3. async client interface
    {
        let db = Walrus::setup(db_path)?;
        let client = db.async_init()?;

        client.put(b"server:status", b"ready").await?;

        let res = client.get(b"server:status").await?;
        println!("got server:status: {}", String::from_utf8_lossy(&res));

        client.del(b"server:status").await?;
        assert!(client.get(b"server:status").await.is_err());
    }

    if db_path.exists() {
        fs::remove_file(db_path)?;
    }

    Ok(())
}
