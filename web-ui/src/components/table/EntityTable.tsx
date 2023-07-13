// The entity table is a wrapper around the DataGridPro component.
//
// It is used to display a list of entities in a table. It's the generic version
// of a table we use to display programs, pipelines, etc.

import { ChangeEvent, useState, useEffect, Dispatch, MutableRefObject } from 'react'
import Card from '@mui/material/Card'
import { DataGridPro, GridValidRowModel, DataGridProProps, GridRenderCellParams } from '@mui/x-data-grid-pro'
import { UseQueryResult } from '@tanstack/react-query'
import { GridApiPro } from '@mui/x-data-grid-pro/models/gridApiPro'
import { ErrorOverlay } from './ErrorOverlay'
import { escapeRegExp } from 'src/utils'
import Tooltip from '@mui/material/Tooltip'
import IconButton from '@mui/material/IconButton'
import { Icon } from '@iconify/react'

import QuickSearchToolbar from 'src/components/table/QuickSearchToolbar'

// This is a workaround for the following issue:
// https://github.com/mui/mui-x/issues/5239
// https://github.com/mui/material-ui/issues/35287#issuecomment-1337250566
declare global {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace React {
    interface DOMAttributes<T> {
      onResize?: ReactEventHandler<T> | undefined
      onResizeCapture?: ReactEventHandler<T> | undefined
      nonce?: string | undefined
    }
  }
}

export type EntityTableProps<TData extends GridValidRowModel> = {
  setRows: (rows: TData[]) => void
  fetchRows: UseQueryResult<TData[], unknown>
  onUpdateRow?: (newRow: TData, oldRow: TData) => TData
  onDeleteRow?: Dispatch<TData>
  onDuplicateClicked?: Dispatch<TData>
  onEditClicked?: Dispatch<TData>
  hasSearch?: boolean
  hasFilter?: boolean
  addActions?: boolean
  tableProps: DataGridProProps<TData>
  apiRef?: MutableRefObject<GridApiPro>
}

const EntityTable = <TData extends GridValidRowModel>(props: EntityTableProps<TData>) => {
  // By default 7 rows are displayed
  const ROWS_DISPLAYED = 7
  // We can choose between these options for the number of rows per page
  const ROWS_PER_PAGE_OPTIONS = [7, 10, 25, 50]

  const { setRows, fetchRows, onUpdateRow, onDeleteRow, onEditClicked, tableProps, addActions } = props

  const [pageSize, setPageSize] = useState<number>(ROWS_DISPLAYED)
  const [searchText, setSearchText] = useState<string>('')
  const [filteredData, setFilteredData] = useState<TData[]>([])

  const { isLoading, isError, data, error } = fetchRows

  if (addActions) {
    tableProps.columns.push({
      flex: 0.1,
      minWidth: 90,
      sortable: false,
      field: 'actions',
      headerName: 'Actions',
      renderCell: (params: GridRenderCellParams) => {
        return (
          <>
            {onEditClicked && (
              <Tooltip title='Edit'>
                <IconButton
                  size='small'
                  onClick={e => {
                    e.preventDefault()
                    onEditClicked(params.row)
                  }}
                >
                  <Icon icon='bx:pencil' fontSize={20} />
                </IconButton>
              </Tooltip>
            )}
            {onDeleteRow && (
              <Tooltip title='Delete'>
                <IconButton size='small' onClick={() => onDeleteRow(params.row)}>
                  <Icon icon='bx:trash-alt' fontSize={20} />
                </IconButton>
              </Tooltip>
            )}
          </>
        )
      }
    })
  }

  useEffect(() => {
    if (!isLoading && !isError) {
      setRows(data)
    }
  }, [isLoading, isError, data, setRows])

  const handleSearch = (searchValue: string) => {
    setSearchText(searchValue)
    if (searchValue.length == 0) {
      setFilteredData([])
      return
    }

    const searchRegex = new RegExp(escapeRegExp(searchValue), 'i')
    if (!isLoading && !isError) {
      const filteredRows = data.filter((row: any) => {
        return Object.keys(row).some(field => {
          // @ts-ignore
          if (row[field] !== null) {
            return searchRegex.test(row[field].toString())
          }
        })
      })
      setFilteredData(filteredRows)
    }
  }

  return (
    <Card>
      <DataGridPro
        {...tableProps}
        autoHeight
        experimentalFeatures={{ newEditingApi: true }}
        apiRef={props.apiRef}
        components={{
          Toolbar: QuickSearchToolbar,
          ErrorOverlay: ErrorOverlay
        }}
        rows={filteredData.length ? filteredData : tableProps.rows}
        pageSize={pageSize}
        rowsPerPageOptions={ROWS_PER_PAGE_OPTIONS}
        onPageSizeChange={newPageSize => setPageSize(newPageSize)}
        processRowUpdate={onUpdateRow}
        error={error}
        loading={isLoading}
        componentsProps={{
          baseButton: {
            variant: 'outlined'
          },
          toolbar: {
            hasSearch: props.hasSearch,
            hasFilter: props.hasFilter,
            value: searchText,
            clearSearch: () => handleSearch(''),
            onChange: (event: ChangeEvent<HTMLInputElement>) => handleSearch(event.target.value)
          },
          errorOverlay: {
            error: error
          }
        }}
      />
    </Card>
  )
}

export default EntityTable
