use std::{any::Any, sync::Arc};

use async_trait::async_trait;
use datafusion::{
    arrow::{
        array::{
            Array, ArrayRef, BooleanBuilder, Int16Builder, ListBuilder, StringBuilder,
            UInt16Builder, UInt32Builder,
        },
        datatypes::{DataType, Field, Schema, SchemaRef},
        record_batch::RecordBatch,
    },
    datasource::{datasource::TableProviderFilterPushDown, TableProvider, TableType},
    error::DataFusionError,
    logical_plan::Expr,
    physical_plan::{memory::MemoryExec, ExecutionPlan},
};

struct PgCatalogIndexBuilder {
    indexrelid: UInt32Builder,
    indrelid: UInt32Builder,
    indnatts: UInt16Builder,
    indnkeyatts: UInt16Builder,
    indisunique: BooleanBuilder,
    indisprimary: BooleanBuilder,
    indisexclusion: BooleanBuilder,
    indimmediate: BooleanBuilder,
    indisclustered: BooleanBuilder,
    indisvalid: BooleanBuilder,
    indcheckxmin: BooleanBuilder,
    indisready: BooleanBuilder,
    indislive: BooleanBuilder,
    indisreplident: BooleanBuilder,
    indkey: ListBuilder<Int16Builder>,
    indcollation: ListBuilder<UInt32Builder>,
    indclass: ListBuilder<UInt32Builder>,
    indoption: ListBuilder<Int16Builder>,
    indexprs: StringBuilder,
    indpred: StringBuilder,
    xmin: UInt32Builder,
}

impl PgCatalogIndexBuilder {
    fn new() -> Self {
        let capacity = 10;

        Self {
            indexrelid: UInt32Builder::new(capacity),
            indrelid: UInt32Builder::new(capacity),
            indnatts: UInt16Builder::new(capacity),
            indnkeyatts: UInt16Builder::new(capacity),
            indisunique: BooleanBuilder::new(capacity),
            indisprimary: BooleanBuilder::new(capacity),
            indisexclusion: BooleanBuilder::new(capacity),
            indimmediate: BooleanBuilder::new(capacity),
            indisclustered: BooleanBuilder::new(capacity),
            indisvalid: BooleanBuilder::new(capacity),
            indcheckxmin: BooleanBuilder::new(capacity),
            indisready: BooleanBuilder::new(capacity),
            indislive: BooleanBuilder::new(capacity),
            indisreplident: BooleanBuilder::new(capacity),
            indkey: ListBuilder::new(Int16Builder::new(capacity)),
            indcollation: ListBuilder::new(UInt32Builder::new(capacity)),
            indclass: ListBuilder::new(UInt32Builder::new(capacity)),
            indoption: ListBuilder::new(Int16Builder::new(capacity)),
            indexprs: StringBuilder::new(capacity),
            indpred: StringBuilder::new(capacity),
            xmin: UInt32Builder::new(capacity),
        }
    }

    fn finish(mut self) -> Vec<Arc<dyn Array>> {
        let columns: Vec<Arc<dyn Array>> = vec![
            Arc::new(self.indexrelid.finish()),
            Arc::new(self.indrelid.finish()),
            Arc::new(self.indnatts.finish()),
            Arc::new(self.indnkeyatts.finish()),
            Arc::new(self.indisunique.finish()),
            Arc::new(self.indisprimary.finish()),
            Arc::new(self.indisexclusion.finish()),
            Arc::new(self.indimmediate.finish()),
            Arc::new(self.indisclustered.finish()),
            Arc::new(self.indisvalid.finish()),
            Arc::new(self.indcheckxmin.finish()),
            Arc::new(self.indisready.finish()),
            Arc::new(self.indislive.finish()),
            Arc::new(self.indisreplident.finish()),
            Arc::new(self.indkey.finish()),
            Arc::new(self.indcollation.finish()),
            Arc::new(self.indclass.finish()),
            Arc::new(self.indoption.finish()),
            Arc::new(self.indexprs.finish()),
            Arc::new(self.indpred.finish()),
            Arc::new(self.xmin.finish()),
        ];

        columns
    }
}

pub struct PgCatalogIndexProvider {
    data: Arc<Vec<ArrayRef>>,
}

impl PgCatalogIndexProvider {
    pub fn new() -> Self {
        let builder = PgCatalogIndexBuilder::new();

        Self {
            data: Arc::new(builder.finish()),
        }
    }
}

#[async_trait]
impl TableProvider for PgCatalogIndexProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    fn schema(&self) -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("indexrelid", DataType::UInt32, false),
            Field::new("indrelid", DataType::UInt32, false),
            Field::new("indnatts", DataType::UInt16, false),
            Field::new("indnkeyatts", DataType::UInt16, false),
            Field::new("indisunique", DataType::Boolean, false),
            Field::new("indisprimary", DataType::Boolean, false),
            Field::new("indisexclusion", DataType::Boolean, false),
            Field::new("indimmediate", DataType::Boolean, false),
            Field::new("indisclustered", DataType::Boolean, false),
            Field::new("indisvalid", DataType::Boolean, false),
            Field::new("indcheckxmin", DataType::Boolean, false),
            Field::new("indisready", DataType::Boolean, false),
            Field::new("indislive", DataType::Boolean, false),
            Field::new("indisreplident", DataType::Boolean, false),
            Field::new(
                "indkey",
                DataType::List(Box::new(Field::new("item", DataType::Int16, true))),
                false,
            ),
            Field::new(
                "indcollation",
                DataType::List(Box::new(Field::new("item", DataType::UInt32, true))),
                false,
            ),
            Field::new(
                "indclass",
                DataType::List(Box::new(Field::new("item", DataType::UInt32, true))),
                false,
            ),
            Field::new(
                "indoption",
                DataType::List(Box::new(Field::new("item", DataType::Int16, true))),
                false,
            ),
            Field::new("indexprs", DataType::Utf8, true),
            Field::new("indpred", DataType::Utf8, true),
            Field::new("xmin", DataType::UInt32, false),
        ]))
    }

    async fn scan(
        &self,
        projection: &Option<Vec<usize>>,
        _filters: &[Expr],
        _limit: Option<usize>,
    ) -> Result<Arc<dyn ExecutionPlan>, DataFusionError> {
        let batch = RecordBatch::try_new(self.schema(), self.data.to_vec())?;

        Ok(Arc::new(MemoryExec::try_new(
            &[vec![batch]],
            self.schema(),
            projection.clone(),
        )?))
    }

    fn supports_filter_pushdown(
        &self,
        _filter: &Expr,
    ) -> Result<TableProviderFilterPushDown, DataFusionError> {
        Ok(TableProviderFilterPushDown::Unsupported)
    }
}
