use std::{any::Any, sync::Arc};

use async_trait::async_trait;

use datafusion::{
    arrow::{
        array::{
            Array, ArrayRef, BooleanBuilder, Int16Builder, Int32Builder, ListBuilder,
            StringBuilder, UInt32Builder,
        },
        datatypes::{DataType, Field, Schema, SchemaRef},
        record_batch::RecordBatch,
    },
    datasource::{datasource::TableProviderFilterPushDown, TableProvider, TableType},
    error::DataFusionError,
    logical_plan::Expr,
    physical_plan::{memory::MemoryExec, ExecutionPlan},
};

struct PgCatalogConstraintBuilder {
    oid: UInt32Builder,
    conname: StringBuilder,
    connamespace: UInt32Builder,
    contype: StringBuilder,
    condeferrable: BooleanBuilder,
    condeferred: BooleanBuilder,
    convalidated: BooleanBuilder,
    conrelid: UInt32Builder,
    contypid: UInt32Builder,
    conindid: UInt32Builder,
    conparentid: UInt32Builder,
    confrelid: UInt32Builder,
    confupdtype: StringBuilder,
    confdeltype: StringBuilder,
    confmatchtype: StringBuilder,
    conislocal: BooleanBuilder,
    coninhcount: Int32Builder,
    connoinherit: BooleanBuilder,
    conkey: ListBuilder<Int16Builder>,
    confkey: ListBuilder<Int16Builder>,
    conpfeqop: StringBuilder,
    conppeqop: StringBuilder,
    conffeqop: StringBuilder,
    conexclop: StringBuilder,
    conbin: StringBuilder,
    xmin: UInt32Builder,
}

impl PgCatalogConstraintBuilder {
    fn new() -> Self {
        let capacity = 10;

        Self {
            oid: UInt32Builder::new(capacity),
            conname: StringBuilder::new(capacity),
            connamespace: UInt32Builder::new(capacity),
            contype: StringBuilder::new(capacity),
            condeferrable: BooleanBuilder::new(capacity),
            condeferred: BooleanBuilder::new(capacity),
            convalidated: BooleanBuilder::new(capacity),
            conrelid: UInt32Builder::new(capacity),
            contypid: UInt32Builder::new(capacity),
            conindid: UInt32Builder::new(capacity),
            conparentid: UInt32Builder::new(capacity),
            confrelid: UInt32Builder::new(capacity),
            confupdtype: StringBuilder::new(capacity),
            confdeltype: StringBuilder::new(capacity),
            confmatchtype: StringBuilder::new(capacity),
            conislocal: BooleanBuilder::new(capacity),
            coninhcount: Int32Builder::new(capacity),
            connoinherit: BooleanBuilder::new(capacity),
            conkey: ListBuilder::new(Int16Builder::new(capacity)),
            confkey: ListBuilder::new(Int16Builder::new(capacity)),
            conpfeqop: StringBuilder::new(capacity),
            conppeqop: StringBuilder::new(capacity),
            conffeqop: StringBuilder::new(capacity),
            conexclop: StringBuilder::new(capacity),
            conbin: StringBuilder::new(capacity),
            xmin: UInt32Builder::new(capacity),
        }
    }

    fn finish(mut self) -> Vec<Arc<dyn Array>> {
        let columns: Vec<Arc<dyn Array>> = vec![
            Arc::new(self.oid.finish()),
            Arc::new(self.conname.finish()),
            Arc::new(self.connamespace.finish()),
            Arc::new(self.contype.finish()),
            Arc::new(self.condeferrable.finish()),
            Arc::new(self.condeferred.finish()),
            Arc::new(self.convalidated.finish()),
            Arc::new(self.conrelid.finish()),
            Arc::new(self.contypid.finish()),
            Arc::new(self.conindid.finish()),
            Arc::new(self.conparentid.finish()),
            Arc::new(self.confrelid.finish()),
            Arc::new(self.confupdtype.finish()),
            Arc::new(self.confdeltype.finish()),
            Arc::new(self.confmatchtype.finish()),
            Arc::new(self.conislocal.finish()),
            Arc::new(self.coninhcount.finish()),
            Arc::new(self.connoinherit.finish()),
            Arc::new(self.conkey.finish()),
            Arc::new(self.confkey.finish()),
            Arc::new(self.conpfeqop.finish()),
            Arc::new(self.conppeqop.finish()),
            Arc::new(self.conffeqop.finish()),
            Arc::new(self.conexclop.finish()),
            Arc::new(self.conbin.finish()),
            Arc::new(self.xmin.finish()),
        ];

        columns
    }
}

pub struct PgCatalogConstraintProvider {
    data: Arc<Vec<ArrayRef>>,
}

impl PgCatalogConstraintProvider {
    pub fn new() -> Self {
        let builder = PgCatalogConstraintBuilder::new();

        Self {
            data: Arc::new(builder.finish()),
        }
    }
}

#[async_trait]
impl TableProvider for PgCatalogConstraintProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn table_type(&self) -> TableType {
        TableType::View
    }

    fn schema(&self) -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("oid", DataType::UInt32, false),
            Field::new("conname", DataType::Utf8, false),
            Field::new("connamespace", DataType::UInt32, false),
            Field::new("contype", DataType::Utf8, false),
            Field::new("condeferrable", DataType::Boolean, false),
            Field::new("condeferred", DataType::Boolean, false),
            Field::new("convalidated", DataType::Boolean, false),
            Field::new("conrelid", DataType::UInt32, false),
            Field::new("contypid", DataType::UInt32, false),
            Field::new("conindid", DataType::UInt32, false),
            Field::new("conparentid", DataType::UInt32, false),
            Field::new("confrelid", DataType::UInt32, false),
            Field::new("confupdtype", DataType::Utf8, false),
            Field::new("confdeltype", DataType::Utf8, false),
            Field::new("confmatchtype", DataType::Utf8, false),
            Field::new("conislocal", DataType::Boolean, false),
            Field::new("coninhcount", DataType::Int32, false),
            Field::new("connoinherit", DataType::Boolean, false),
            Field::new(
                "conkey",
                DataType::List(Box::new(Field::new("item", DataType::Int16, true))),
                true,
            ),
            Field::new(
                "confkey",
                DataType::List(Box::new(Field::new("item", DataType::Int16, true))),
                true,
            ),
            Field::new("conpfeqop", DataType::Utf8, true),
            Field::new("conppeqop", DataType::Utf8, true),
            Field::new("conffeqop", DataType::Utf8, true),
            Field::new("conexclop", DataType::Utf8, true),
            Field::new("conbin", DataType::Utf8, true),
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
