import { DataGrid, GridColDef, GridRowId } from "@mui/x-data-grid";

export interface DbDataGridProps {
  rows: any[];
  columns: GridColDef[];
  getRowId?: (row: any) => GridRowId;
  pageSize?: number;
  onSelectedRowsChange?: (selectedRow: GridRowId | undefined) => void;
}

function DbDataGrid(props: DbDataGridProps) {
  const { rows, columns } = props;

  return (
    <DataGrid
      rows={rows}
      columns={columns}
      getRowId={props.getRowId}
      initialState={{
        pagination: {
          paginationModel: {
            pageSize: props.pageSize || 20,
          },
        },
      }}
      pageSizeOptions={[5]}
      disableMultipleRowSelection
      onRowSelectionModelChange={(selections) => {
        props.onSelectedRowsChange?.(selections[0]);
      }}
      checkboxSelection
    />
  );
}

export default DbDataGrid;
