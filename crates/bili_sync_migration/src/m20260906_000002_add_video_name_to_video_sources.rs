use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let manager_tables = [
            Favorite::Table.into_table_ref(),
            Collection::Table.into_table_ref(),
            Submission::Table.into_table_ref(),
            WatchLater::Table.into_table_ref(),
            NormalVideo::Table.into_table_ref(),
        ];
        for table in manager_tables {
            manager
                .alter_table(
                    Table::alter()
                        .table(table)
                        .add_column(ColumnDef::new(Alias::new("video_name")).string().null())
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let manager_tables = [
            Favorite::Table.into_table_ref(),
            Collection::Table.into_table_ref(),
            Submission::Table.into_table_ref(),
            WatchLater::Table.into_table_ref(),
            NormalVideo::Table.into_table_ref(),
        ];
        for table in manager_tables {
            manager
                .alter_table(
                    Table::alter()
                        .table(table)
                        .drop_column(Alias::new("video_name"))
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Favorite {
    Table,
}

#[derive(DeriveIden)]
enum Collection {
    Table,
}

#[derive(DeriveIden)]
enum Submission {
    Table,
}

#[derive(DeriveIden)]
enum WatchLater {
    Table,
}

#[derive(DeriveIden)]
enum NormalVideo {
    Table,
}
