import { useState, useEffect, SetStateAction, Dispatch } from 'react'
import YAML from 'yaml'
import { useForm } from 'react-hook-form'
import { yupResolver } from '@hookform/resolvers/yup'
import * as yup from 'yup'

import Box from '@mui/material/Box'
import Tab from '@mui/material/Tab'
import Card from '@mui/material/Card'
import TabList from '@mui/lab/TabList'
import TabPanel from '@mui/lab/TabPanel'
import Dialog from '@mui/material/Dialog'
import Button from '@mui/material/Button'
import TabContext from '@mui/lab/TabContext'
import IconButton from '@mui/material/IconButton'
import Typography from '@mui/material/Typography'
import CardContent from '@mui/material/CardContent'
import DialogContent from '@mui/material/DialogContent'

import { Icon } from '@iconify/react'
import DialogTabDetails from 'src/data/create-app-tabs/DialogTabDetails'
import TabFooter from 'src/data/create-app-tabs/TabFooter'
import TabLabel from 'src/data/create-app-tabs/TabLabel'
import { ConnectorType, ConnectorDescr } from 'src/types/manager'
import Transition from './create-app-tabs/Transition'
import { SourceFormCreateHandle } from './SubmitHandler'
import { connectorTypeToConfig } from 'src/types/data'
import TabKafkaOutDetails from './create-app-tabs/TabKafkaOutDetails'

const schema = yup
  .object({
    name: yup.string().required(),
    description: yup.string().default(''),
    host: yup.string().required(),
    auto_offset: yup.string().default('earliest'),
    topic: yup.string()
  })
  .required()

export type KafkaOutConnector = yup.InferType<typeof schema>

export const DialogCreateKafkaOut = (props: {
  show: boolean
  setShow: Dispatch<SetStateAction<boolean>>
  onSuccess?: Dispatch<ConnectorDescr>
}) => {
  const [activeTab, setActiveTab] = useState<string>('detailsTab')
  const handleClose = () => {
    props.setShow(false)
  }

  const onFormSubmitted = (descr: ConnectorDescr | undefined) => {
    handleClose()
    if (descr !== undefined && props.onSuccess !== undefined) {
      props.onSuccess(descr)
    }
  }

  const {
    control,
    handleSubmit,
    formState: { errors }
  } = useForm({
    resolver: yupResolver(schema),
    defaultValues: {
      name: '',
      description: '',
      host: '',
      auto_offset: 'earliest'
    }
  })

  useEffect(() => {
    // If we have an error in the details tab, switch to the details tab
    if ((errors?.name || errors?.description) && props.show) {
      setActiveTab('detailsTab')
    }
  }, [props.show, errors])

  // Add a new kafka source
  const onSubmit = SourceFormCreateHandle<KafkaOutConnector>(onFormSubmitted, data => {
    return {
      name: data.name,
      description: data.description,
      typ: ConnectorType.KAFKA_OUT,
      config: YAML.stringify({
        transport: {
          name: connectorTypeToConfig(ConnectorType.KAFKA_OUT),
          config: {
            'bootstrap.servers': data.host,
            'auto.offset.reset': data.auto_offset,
            topic: data.topic
          }
        },
        format: { name: 'csv' }
      })
    }
  })

  return (
    <Dialog
      fullWidth
      open={props.show}
      scroll='body'
      maxWidth='md'
      onClose={handleClose}
      TransitionComponent={Transition}
    >
      <form id='create-kafka' onSubmit={handleSubmit(onSubmit)}>
        <DialogContent
          sx={{
            pt: { xs: 8, sm: 12.5 },
            pr: { xs: 5, sm: 12 },
            pb: { xs: 5, sm: 9.5 },
            pl: { xs: 4, sm: 11 },
            position: 'relative'
          }}
        >
          <IconButton size='small' onClick={handleClose} sx={{ position: 'absolute', right: '1rem', top: '1rem' }}>
            <Icon icon='bx:x' />
          </IconButton>
          <Box sx={{ mb: 8, textAlign: 'center' }}>
            <Typography variant='h5' sx={{ mb: 3 }}>
              New Kafka Output
            </Typography>
            <Typography variant='body2'>Add a Kafka or Repanda output.</Typography>
          </Box>
          <Box sx={{ display: 'flex', flexWrap: { xs: 'wrap', md: 'nowrap' } }}>
            <TabContext value={activeTab}>
              <TabList
                orientation='vertical'
                onChange={(e, newValue: string) => setActiveTab(newValue)}
                sx={{
                  border: 0,
                  minWidth: 200,
                  '& .MuiTabs-indicator': { display: 'none' },
                  '& .MuiTabs-flexContainer': {
                    alignItems: 'flex-start',
                    '& .MuiTab-root': {
                      width: '100%',
                      alignItems: 'flex-start'
                    }
                  }
                }}
              >
                <Tab
                  disableRipple
                  value='detailsTab'
                  label={
                    <TabLabel
                      title='Details'
                      subtitle='Enter Details'
                      active={activeTab === 'detailsTab'}
                      icon={<Icon icon='bx:file' />}
                    />
                  }
                />
                <Tab
                  disableRipple
                  value='sourceTab'
                  label={
                    <TabLabel
                      title='Server'
                      active={activeTab === 'sourceTab'}
                      subtitle='Source details'
                      icon={<Icon icon='bx:data' />}
                    />
                  }
                />
              </TabList>
              <TabPanel
                value='detailsTab'
                sx={{ border: 0, boxShadow: 0, width: '100%', backgroundColor: 'transparent' }}
              >
                <DialogTabDetails control={control} errors={errors} />
                <TabFooter
                  activeTab={activeTab}
                  setActiveTab={setActiveTab}
                  formId='create-kafka'
                  tabsArr={['detailsTab', 'sourceTab']}
                />
              </TabPanel>
              <TabPanel
                value='sourceTab'
                sx={{ border: 0, boxShadow: 0, width: '100%', backgroundColor: 'transparent' }}
              >
                <TabKafkaOutDetails control={control} errors={errors} />
                <TabFooter
                  activeTab={activeTab}
                  setActiveTab={setActiveTab}
                  formId='create-kafka'
                  tabsArr={['detailsTab', 'sourceTab']}
                />
              </TabPanel>
            </TabContext>
          </Box>
        </DialogContent>
      </form>
    </Dialog>
  )
}

const DialogCreateKafkaOutBox = () => {
  const [show, setShow] = useState<boolean>(false)

  return (
    <Card>
      <CardContent sx={{ textAlign: 'center', '& svg': { mb: 2 } }}>
        <Icon icon='logos:kafka' fontSize='4rem' />
        <Typography sx={{ mb: 3 }}>Add a Kafka Output.</Typography>
        <Button variant='contained' onClick={() => setShow(true)}>
          Add
        </Button>
      </CardContent>
      <DialogCreateKafkaOut show={show} setShow={setShow} />
    </Card>
  )
}

export default DialogCreateKafkaOutBox
