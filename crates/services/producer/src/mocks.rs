use crate::ports::{
    BlockProducerDatabase,
    Executor,
    Relayer,
    TxPool,
};
use fuel_core_storage::{
    transactional::{
        StorageTransaction,
        Transactional,
    },
    Result as StorageResult,
};
use fuel_core_types::{
    blockchain::{
        block::CompressedBlock,
        primitives::{
            BlockHeight,
            DaBlockHeight,
        },
    },
    entities::message::Message,
    fuel_tx::Receipt,
    fuel_types::{
        Address,
        MessageId,
    },
    services::{
        executor::{
            Error as ExecutorError,
            ExecutionBlock,
            ExecutionResult,
            Result as ExecutorResult,
            UncommittedResult,
        },
        txpool::{
            ArcPoolTx,
            Error as TxPoolError,
        },
    },
};
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{
        Arc,
        Mutex,
    },
};

#[derive(Default, Clone)]
pub struct MockRelayer {
    pub block_production_key: Address,
    pub best_finalized_height: DaBlockHeight,
}

#[async_trait::async_trait]
impl Relayer for MockRelayer {
    /// Get the best finalized height from the DA layer
    async fn get_best_finalized_da_height(&self) -> StorageResult<DaBlockHeight> {
        Ok(self.best_finalized_height)
    }
}

#[derive(Default)]
pub struct MockTxPool(pub Vec<ArcPoolTx>);

#[async_trait::async_trait]
impl TxPool for MockTxPool {
    async fn get_includable_txs(
        &self,
        _block_height: BlockHeight,
        _max_gas: u64,
    ) -> Result<Vec<ArcPoolTx>, TxPoolError> {
        Ok(self.0.clone().into_iter().collect())
    }
}

#[derive(Default)]
pub struct MockExecutor(pub MockDb);

#[derive(Debug)]
struct DatabaseTransaction {
    database: MockDb,
}

impl Transactional<MockDb> for DatabaseTransaction {
    fn commit(&mut self) -> StorageResult<()> {
        Ok(())
    }
}

impl AsMut<MockDb> for DatabaseTransaction {
    fn as_mut(&mut self) -> &mut MockDb {
        &mut self.database
    }
}

impl AsRef<MockDb> for DatabaseTransaction {
    fn as_ref(&self) -> &MockDb {
        &self.database
    }
}

impl Transactional<MockDb> for MockDb {
    fn commit(&mut self) -> StorageResult<()> {
        Ok(())
    }
}

impl AsMut<MockDb> for MockDb {
    fn as_mut(&mut self) -> &mut MockDb {
        self
    }
}

impl AsRef<MockDb> for MockDb {
    fn as_ref(&self) -> &MockDb {
        self
    }
}

impl Executor<MockDb> for MockExecutor {
    fn execute_without_commit(
        &self,
        block: ExecutionBlock,
    ) -> ExecutorResult<UncommittedResult<StorageTransaction<MockDb>>> {
        let block = match block {
            ExecutionBlock::Production(block) => block.generate(&[]),
            ExecutionBlock::Validation(block) => block,
        };
        // simulate executor inserting a block
        let mut block_db = self.0.blocks.lock().unwrap();
        block_db.insert(*block.header().height(), block.compress());
        Ok(UncommittedResult::new(
            ExecutionResult {
                block,
                skipped_transactions: vec![],
                tx_status: vec![],
            },
            StorageTransaction::new(self.0.clone()),
        ))
    }

    fn dry_run(
        &self,
        _block: ExecutionBlock,
        _utxo_validation: Option<bool>,
    ) -> ExecutorResult<Vec<Vec<Receipt>>> {
        Ok(Default::default())
    }
}

pub struct FailingMockExecutor(pub Mutex<Option<ExecutorError>>);

impl Executor<MockDb> for FailingMockExecutor {
    fn execute_without_commit(
        &self,
        block: ExecutionBlock,
    ) -> ExecutorResult<UncommittedResult<StorageTransaction<MockDb>>> {
        // simulate an execution failure
        let mut err = self.0.lock().unwrap();
        if let Some(err) = err.take() {
            Err(err)
        } else {
            let block = match block {
                ExecutionBlock::Production(b) => b.generate(&[]),
                ExecutionBlock::Validation(b) => b,
            };
            Ok(UncommittedResult::new(
                ExecutionResult {
                    block,
                    skipped_transactions: vec![],
                    tx_status: vec![],
                },
                StorageTransaction::new(MockDb::default()),
            ))
        }
    }

    fn dry_run(
        &self,
        _block: ExecutionBlock,
        _utxo_validation: Option<bool>,
    ) -> ExecutorResult<Vec<Vec<Receipt>>> {
        let mut err = self.0.lock().unwrap();
        if let Some(err) = err.take() {
            Err(err)
        } else {
            Ok(Default::default())
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct MockDb {
    pub blocks: Arc<Mutex<HashMap<BlockHeight, CompressedBlock>>>,
    pub messages: Arc<Mutex<HashMap<MessageId, Message>>>,
}

impl BlockProducerDatabase for MockDb {
    fn get_block(
        &self,
        fuel_height: BlockHeight,
    ) -> StorageResult<Option<Cow<CompressedBlock>>> {
        let blocks = self.blocks.lock().unwrap();

        Ok(blocks.get(&fuel_height).cloned().map(Cow::Owned))
    }

    fn current_block_height(&self) -> StorageResult<BlockHeight> {
        let blocks = self.blocks.lock().unwrap();

        Ok(blocks.keys().max().cloned().unwrap_or_default())
    }
}
