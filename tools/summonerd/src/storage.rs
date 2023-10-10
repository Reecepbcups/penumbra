use anyhow::Result;
use camino::Utf8Path;
use penumbra_keys::Address;
use penumbra_num::Amount;
use penumbra_proof_setup::all::{
    Phase2CeremonyCRS, Phase2CeremonyContribution, Phase2RawCeremonyCRS,
    Phase2RawCeremonyContribution,
};
use penumbra_proto::{
    penumbra::tools::summoning::v1alpha1::{
        self as pb, participate_request::Contribution as PBContribution,
    },
    Message,
};
use r2d2_sqlite::{rusqlite::OpenFlags, SqliteConnectionManager};
use tokio::task::spawn_blocking;

use crate::penumbra_knower::PenumbraKnower;

const MIN_BID_AMOUNT_U64: u64 = 1u64;

#[derive(Clone)]
pub struct Storage {
    pool: r2d2::Pool<SqliteConnectionManager>,
}

impl Storage {
    /// If the database at `storage_path` exists, [`Self::load`] it, otherwise, [`Self::initialize`] it.
    pub async fn load_or_initialize(storage_path: impl AsRef<Utf8Path>) -> anyhow::Result<Self> {
        if storage_path.as_ref().exists() {
            return Self::load(storage_path).await;
        }

        Self::initialize(storage_path).await
    }

    pub async fn initialize(storage_path: impl AsRef<Utf8Path>) -> anyhow::Result<Self> {
        // Connect to the database (or create it)
        let pool = Self::connect(storage_path)?;

        spawn_blocking(move || {
            // In one database transaction, populate everything
            let mut conn = pool.get()?;
            let tx = conn.transaction()?;

            // Create the tables
            tx.execute_batch(include_str!("storage/schema.sql"))?;
            // TODO: Remove this in favor of a specific command for initializing root
            let root = Phase2CeremonyCRS::root()?;
            tx.execute(
                "INSERT INTO phase2_contributions VALUES (0, 1, ?1, NULL)",
                [pb::CeremonyCrs::try_from(root)?.encode_to_vec()],
            )?;

            tx.commit()?;

            Ok(Storage { pool })
        })
        .await?
    }

    fn connect(path: impl AsRef<Utf8Path>) -> anyhow::Result<r2d2::Pool<SqliteConnectionManager>> {
        let manager = SqliteConnectionManager::file(path.as_ref())
            .with_flags(
                // Don't allow opening URIs, because they can change the behavior of the database; we
                // just want to open normal filepaths.
                OpenFlags::default() & !OpenFlags::SQLITE_OPEN_URI,
            )
            .with_init(|conn| {
                // We use `prepare_cached` a fair amount: this is an overestimate of the number
                // of cached prepared statements likely to be used.
                conn.set_prepared_statement_cache_capacity(32);
                Ok(())
            });
        Ok(r2d2::Pool::new(manager)?)
    }

    pub async fn load(path: impl AsRef<Utf8Path>) -> anyhow::Result<Self> {
        let storage = Self {
            pool: Self::connect(path)?,
        };

        Ok(storage)
    }

    /// Check if a participant can contribute.
    ///
    /// If they can't, None will be returned, otherwise we'll have Some(amount),
    /// with the amount indicating their bid, which can be useful for ranking.
    pub async fn can_contribute(
        &self,
        knower: &PenumbraKnower,
        address: &Address,
    ) -> Result<Option<Amount>> {
        // Criteria:
        // - Not banned TODO
        // - Bid more than min amount
        // - Hasn't already contributed TODO
        let amount = knower.total_amount_sent_to_me(&address).await?;
        if amount < Amount::from(MIN_BID_AMOUNT_U64) {
            return Ok(None);
        }
        Ok(Some(amount))
    }

    pub async fn current_crs(&self) -> Result<Phase2CeremonyCRS> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        let (is_root, contribution_or_crs) = tx.query_row(
            "SELECT is_root, contribution_or_crs FROM phase2_contributions ORDER BY slot DESC LIMIT 1",
            [],
            |row| Ok((row.get::<usize, bool>(0)?, row.get::<usize, Vec<u8>>(1)?)),
        )?;
        let crs = if is_root {
            Phase2RawCeremonyCRS::try_from(pb::CeremonyCrs::decode(
                contribution_or_crs.as_slice(),
            )?)?
            .assume_valid()
        } else {
            Phase2RawCeremonyContribution::try_from(PBContribution::decode(
                contribution_or_crs.as_slice(),
            )?)?
            .assume_valid()
            .new_elements()
        };
        Ok(crs)
    }

    pub async fn commit_contribution(
        &self,
        contributor: Address,
        contribution: Phase2CeremonyContribution,
    ) -> Result<()> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        let contributor_bytes = contributor.to_vec();
        tx.execute(
            "INSERT INTO phase2_contributions VALUES(NULL, 0, ?1, ?2)",
            [
                PBContribution::try_from(contribution)?.encode_to_vec(),
                contributor_bytes,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub async fn current_slot(&self) -> Result<u64> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        let out = tx
            .query_row("SELECT MAX(slot) FROM phase2_contributions", [], |row| {
                row.get::<usize, Option<u64>>(0)
            })?
            .unwrap_or(0);
        Ok(out)
    }

    pub async fn root(&self) -> Result<Phase2CeremonyCRS> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        let data = tx.query_row(
            "SELECT contribution_or_crs FROM phase2_contributions WHERE is_root LIMIT 1",
            [],
            |row| row.get::<usize, Vec<u8>>(0),
        )?;
        Ok(
            Phase2RawCeremonyCRS::try_from(pb::CeremonyCrs::decode(data.as_slice())?)?
                .assume_valid(),
        )
    }
}