#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::{Seek, SeekFrom, Write};
    use std::path::PathBuf;
    use walrus::{Walror, Walrus};

    struct TestEnv {
        path: PathBuf,
    }

    impl TestEnv {
        fn new(name: &str) -> Self {
            let path = PathBuf::from(format!("target/test_{}_{}.db", name, std::process::id()));
            if path.exists() {
                let _ = fs::remove_file(&path);
            }
            Self { path }
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_file(self.path.with_extension("tmp"));
        }
    }

    #[test]
    fn test_put_get_del_basic() -> Result<(), Walror> {
        let env = TestEnv::new("basic");
        let mut db = Walrus::setup(&env.path)?;

        db.put(b"k1", b"v1")?;
        assert_eq!(db.get(b"k1")?, b"v1");

        db.put(b"k1", b"v2")?;
        assert_eq!(db.get(b"k1")?, b"v2");

        db.del(b"k1")?;
        assert!(matches!(db.get(b"k1"), Err(Walror::NullKey)));

        assert!(db.del(b"nonexistent").is_ok());

        Ok(())
    }

    #[test]
    fn test_replay_recovery() -> Result<(), Walror> {
        let env = TestEnv::new("recovery");

        {
            let mut db = Walrus::setup(&env.path)?;
            db.put(b"alpha", b"100")?;
            db.put(b"beta", b"200")?;
            db.put(b"gamma", b"300")?;
            db.del(b"beta")?;
            db.put(b"alpha", b"150")?;
        }

        let mut db = Walrus::setup(&env.path)?;
        assert_eq!(db.get(b"alpha")?, b"150");
        assert_eq!(db.get(b"gamma")?, b"300");
        assert!(matches!(db.get(b"beta"), Err(Walror::NullKey)));

        Ok(())
    }

    #[test]
    fn test_compaction() -> Result<(), Walror> {
        let env = TestEnv::new("compaction");
        let mut db = Walrus::setup(&env.path)?;

        for i in 0..50 {
            db.put(b"counter", format!("{i}").as_bytes())?;
        }
        db.put(b"persistent", b"data")?;
        db.put(b"temp", b"to_delete")?;
        db.del(b"temp")?;

        let size_before = fs::metadata(&env.path)?.len();

        db.compact()?;

        let size_after = fs::metadata(&env.path)?.len();
        assert!(size_after < size_before);

        assert_eq!(db.get(b"counter")?, b"49");
        assert_eq!(db.get(b"persistent")?, b"data");
        assert!(matches!(db.get(b"temp"), Err(Walror::NullKey)));

        drop(db);
        let mut reloaded = Walrus::setup(&env.path)?;
        assert_eq!(reloaded.get(b"counter")?, b"49");
        assert_eq!(reloaded.get(b"persistent")?, b"data");

        Ok(())
    }

    #[test]
    fn test_crc_mismatch_detection() -> Result<(), Box<dyn std::error::Error>> {
        let env = TestEnv::new("crc");

        {
            let mut db = Walrus::setup(&env.path)?;
            db.put(b"secure_key", b"secure_val")?;
        }

        let mut f = OpenOptions::new().read(true).write(true).open(&env.path)?;
        let file_len = f.metadata()?.len();
        f.seek(SeekFrom::Start(file_len - 1))?;
        f.write_all(&[0xFF])?;
        drop(f);

        let res = Walrus::setup(&env.path);
        assert!(matches!(res, Err(Walror::CrcMismatch)));

        Ok(())
    }

    #[tokio::test]
    async fn test_async_actor_loop() -> Result<(), Box<dyn std::error::Error>> {
        let env = TestEnv::new("async");
        let db = Walrus::setup(&env.path)?;
        let client = db.async_init()?;

        // put & get
        client.put(b"async_k", b"async_v").await?;
        assert_eq!(client.get(b"async_k").await?, b"async_v");

        // compact
        client.compact().await?;
        assert_eq!(client.get(b"async_k").await?, b"async_v");

        // del & verify null
        client.del(b"async_k").await?;
        assert!(matches!(client.get(b"async_k").await, Err(Walror::NullKey)));

        // test client clone works across concurrent tasks
        let client_clone = client.clone();
        let handle = tokio::spawn(async move {
            client_clone.put(b"task_k", b"task_v").await.unwrap();
        });
        handle.await?;

        assert_eq!(client.get(b"task_k").await?, b"task_v");

        Ok(())
    }
}
