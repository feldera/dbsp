// See the status of a input table or output view.
//
// Note: This is still a work in progress and currently does not work as well as
// it should or is not very flexible in displaying what a user wants.
import Grid from '@mui/material/Grid'
import Typography from '@mui/material/Typography'
import { useQuery } from '@tanstack/react-query'
import { useRouter } from 'next/router'
import { useEffect, useState } from 'react'
import PageHeader from 'src/layouts/components/page-header'
import { PipelineDescr, PipelineId, PipelineRevision } from 'src/types/manager'
import { IntrospectionTable } from 'src/streaming/introspection/IntrospectionTable'
import {
  Breadcrumbs,
  FormControl,
  InputLabel,
  Link,
  ListSubheader,
  MenuItem,
  Select,
  SelectChangeEvent
} from '@mui/material'
import { Icon } from '@iconify/react'
import { Controller, useForm } from 'react-hook-form'
import { ErrorBoundary } from 'react-error-boundary'
import { ErrorOverlay } from 'src/components/table/ErrorOverlay'

const TitleBreadCrumb = (props: { pipeline: PipelineDescr; relation: string }) => {
  const pipeline_id = props.pipeline.pipeline_id
  const pipelineRevisionQuery = useQuery<PipelineRevision>(['pipelineLastRevision', { pipeline_id: pipeline_id }])
  const [tables, setTables] = useState<string[]>([])
  const [views, setViews] = useState<string[]>([])

  const router = useRouter()
  const view = router.query.view

  useEffect(() => {
    if (!pipelineRevisionQuery.isLoading && !pipelineRevisionQuery.isError) {
      const pipelineRevision = pipelineRevisionQuery.data
      const program = pipelineRevision?.program
      setTables(program?.schema?.inputs.map(v => v.name) || [])
      setViews(program?.schema?.outputs.map(v => v.name) || [])
    }
  }, [pipelineRevisionQuery.isLoading, pipelineRevisionQuery.isError, pipelineRevisionQuery.data])

  const switchRelation = (e: SelectChangeEvent<string>) => {
    router.push(`/streaming/introspection/${pipeline_id}/${e.target.value}`)
  }

  interface IFormInputs {
    relation: string
  }

  const { control } = useForm<IFormInputs>({
    defaultValues: {
      relation: view as string
    }
  })

  return typeof view === 'string' && tables.length > 0 && views.length > 0 ? (
    <Breadcrumbs separator={<Icon icon='bx:chevron-right' fontSize={20} />} aria-label='breadcrumb'>
      <Link href='/streaming/management/'>{props.pipeline.name}</Link>
      <Controller
        name='relation'
        control={control}
        defaultValue={view}
        render={({ field: { onChange, value } }) => {
          return (
            <FormControl>
              <InputLabel htmlFor='relation-select'>Relation</InputLabel>
              <Select
                label='Select Relation'
                id='relation-select'
                onChange={e => {
                  e.preventDefault()
                  switchRelation(e)
                  onChange(e)
                }}
                value={value}
              >
                <ListSubheader>Tables</ListSubheader>
                {tables.map(item => (
                  <MenuItem key={item} value={item}>
                    {item}
                  </MenuItem>
                ))}
                <ListSubheader>Views</ListSubheader>
                {views.map(item => (
                  <MenuItem key={item} value={item}>
                    {item}
                  </MenuItem>
                ))}
              </Select>
            </FormControl>
          )
        }}
      />
    </Breadcrumbs>
  ) : (
    <></>
  )
}

const IntrospectInputOutput = () => {
  const [pipelineId, setPipelineId] = useState<PipelineId | undefined>(undefined)
  const [relation, setRelation] = useState<string | undefined>(undefined)
  const router = useRouter()

  useEffect(() => {
    if (!router.isReady) {
      return
    }
    const { config, view } = router.query
    if (typeof config === 'string') {
      setPipelineId(config)
    }
    if (typeof view === 'string') {
      setRelation(view)
      console.log('setRelation', view)
    }
  }, [pipelineId, setPipelineId, setRelation, router])
  const [pipelineDescr, setPipelineDescr] = useState<PipelineDescr | undefined>(undefined)
  const configQuery = useQuery<PipelineDescr>(['pipelineStatus', { pipeline_id: pipelineId }], {
    enabled: pipelineId !== undefined
  })
  useEffect(() => {
    if (!configQuery.isLoading && !configQuery.isError) {
      setPipelineDescr(configQuery.data)
    }
  }, [configQuery.isLoading, configQuery.isError, configQuery.data, setPipelineDescr])

  const logError = (error: Error, info: { componentStack: string }) => {
    console.error('ErrorBoundary: ', error, info)
  }

  return (
    !configQuery.isLoading &&
    !configQuery.isError &&
    pipelineDescr &&
    relation &&
    pipelineId !== undefined && (
      <Grid container spacing={6} className='match-height'>
        <PageHeader
          title={<TitleBreadCrumb pipeline={pipelineDescr} relation={relation} />}
          subtitle={<Typography variant='body2'>Introspection</Typography>}
        />

        <Grid item xs={12}>
          <ErrorBoundary FallbackComponent={ErrorOverlay} onError={logError}>
            <IntrospectionTable pipelineDescr={pipelineDescr} name={relation} />
          </ErrorBoundary>
        </Grid>
      </Grid>
    )
  )
}

export default IntrospectInputOutput
