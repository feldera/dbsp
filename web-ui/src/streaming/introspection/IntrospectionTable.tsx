// Browse the contents of a table or a view.

import Card from '@mui/material/Card'
import { DataGridPro, GridColDef, GridPaginationModel, useGridApiRef } from '@mui/x-data-grid-pro'
import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'
import { PipelineDescr, OpenAPI, PipelineRevision, NeighborhoodQuery } from 'src/types/manager'
import useTableUpdater from './hooks/useTableUpdater'
import { useAsyncError } from 'src/utils'

const PAGE_SIZE = 25

export type IntrospectionTableProps = {
  pipelineDescr: PipelineDescr
  name: string
}

export interface IntrospectionTableState {
  headers: GridColDef[]
}

export const IntrospectionTable = ({ pipelineDescr, name }: IntrospectionTableProps) => {
  const apiRef = useGridApiRef()
  const [tableConfig, setTableConfig] = useState<IntrospectionTableState | undefined>(undefined)
  const [isLoading, setLoading] = useState<boolean>(true)
  const [neighborhood, setNeighborhood] = useState<NeighborhoodQuery>({ before: 0, after: PAGE_SIZE - 1 })

  const tableUpdater = useTableUpdater()

  const throwError = useAsyncError()
  const pipelineRevisionQuery = useQuery<PipelineRevision>(
    ['pipelineLastRevision', { pipeline_id: pipelineDescr?.pipeline_id }],
    {
      enabled: pipelineDescr.program_id !== undefined
    }
  )
  useEffect(() => {
    if (!pipelineRevisionQuery.isLoading && !pipelineRevisionQuery.isError && pipelineRevisionQuery.data) {
      const pipelineRevision = pipelineRevisionQuery.data
      const program = pipelineRevision.program
      const tables = program.schema?.inputs.find(v => v.name === name)
      const views = program.schema?.outputs.find(v => v.name === name)
      const relation = tables || views // name is unique in the schema
      if (!relation) {
        return
      }

      const id = [{ field: 'genId', headerName: 'genId' }]
      setTableConfig({
        headers: id
          .concat(
            relation.fields.map((col: any) => {
              return { field: col.name, headerName: col.name, flex: 1 }
            })
          )
          .concat([{ field: 'weight', headerName: 'weight' }])
      })
    }
  }, [pipelineRevisionQuery.isLoading, pipelineRevisionQuery.isError, pipelineRevisionQuery.data, name])

  // Stream changes from backend and update the table
  useEffect(() => {
    if (tableConfig !== undefined && apiRef.current) {
      const controller = new AbortController()
      const url = new URL(
        OpenAPI.BASE +
          '/v0/pipelines/' +
          pipelineDescr.pipeline_id +
          '/egress/' +
          name +
          '?format=csv&query=neighborhood&mode=watch'
      )
      tableUpdater(url, neighborhood, apiRef, tableConfig, controller, setLoading).then(
        () => {
          // nothing to do here, the tableUpdater will update the table
        },
        (error: any) => {
          if (error.name != 'AbortError') {
            throwError(error)
          } else {
            // The AbortError is expected when we leave the view
            // (controller.abort() below will trigger it)
            // -- so nothing to do here
          }
        }
      )

      return () => {
        // If we leave the view, we abort the fetch request otherwise it remains
        // active (the browser and backend only keeps a limited number of active
        // requests and we don't want to exhaust that limit)
        controller.abort()
      }
    }
  }, [pipelineDescr, name, apiRef, tableConfig, tableUpdater, throwError, neighborhood])

  const handlePaginationModelChange = (newPaginationModel: GridPaginationModel) => {
    console.log('handlePaginationModelChange', newPaginationModel)
    setPaginationModel(newPaginationModel)
    setNeighborhood({
      before: 0,
      after: PAGE_SIZE - 1
    })
    // We have the cursor, we can allow the page transition.
    //if (newPaginationModel.page === 0 || mapPageToNextCursor.current[newPaginationModel.page - 1]) {
    //  setPaginationModel(newPaginationModel)
    //}
  }
  const [paginationModel, setPaginationModel] = useState({
    pageSize: PAGE_SIZE,
    page: 0
  })

  return tableConfig ? (
    <Card>
      <DataGridPro
        autoHeight
        pagination
        columnVisibilityModel={{ genId: false, weight: false }}
        getRowId={(row: any) => row.genId}
        apiRef={apiRef}
        columns={tableConfig.headers}
        loading={isLoading}
        rows={[]}
        paginationMode='server'
        pageSizeOptions={[PAGE_SIZE]}
        onPaginationModelChange={handlePaginationModelChange}
        paginationModel={paginationModel}
        // Next two lines are a work-around because DataGridPro needs an
        // accurate rowCount for pagination to work which we don't have.
        //
        // This can be removed once the following issue is resolved:
        // https://github.com/mui/mui-x/issues/409
        rowCount={Number.MAX_VALUE}
        //sx={{
        //  '.MuiTablePagination-displayedRows': {
        //    display: 'none' // 👈 hide huge pagination number
        //  }
        //}}
      />
    </Card>
  ) : (
    <></>
  )
}
