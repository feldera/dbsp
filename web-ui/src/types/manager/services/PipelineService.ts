/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { Chunk } from '../models/Chunk'
import type { EgressMode } from '../models/EgressMode'
import type { NeighborhoodQuery } from '../models/NeighborhoodQuery'
import type { NewPipelineRequest } from '../models/NewPipelineRequest'
import type { NewPipelineResponse } from '../models/NewPipelineResponse'
import type { OutputQuery } from '../models/OutputQuery'
import type { PipelineDescr } from '../models/PipelineDescr'
import type { PipelineRevision } from '../models/PipelineRevision'
import type { UpdatePipelineRequest } from '../models/UpdatePipelineRequest'
import type { UpdatePipelineResponse } from '../models/UpdatePipelineResponse'

import type { CancelablePromise } from '../core/CancelablePromise'
import { OpenAPI } from '../core/OpenAPI'
import { request as __request } from '../core/request'

export class PipelineService {
  /**
   * Retrieve pipeline metadata.
   * Retrieve pipeline metadata.
   * @param id Unique pipeline identifier
   * @param name Unique pipeline name
   * @param toml Set to true to request the configuration of the pipeline as a toml file.
   * @returns string Pipeline descriptor retrieved successfully.
   * @throws ApiError
   */
  public static pipelineStatus(
    id?: string | null,
    name?: string | null,
    toml?: boolean | null
  ): CancelablePromise<string> {
    return __request(OpenAPI, {
      method: 'GET',
      url: '/v0/pipeline',
      query: {
        id: id,
        name: name,
        toml: toml
      },
      errors: {
        400: `Pipeline not specified. Use ?id or ?name query strings in the URL.`,
        404: `Specified pipeline name does not exist in the database.`
      }
    })
  }

  /**
   * List pipelines.
   * List pipelines.
   * @returns PipelineDescr Pipeline list retrieved successfully.
   * @throws ApiError
   */
  public static listPipelines(): CancelablePromise<Array<PipelineDescr>> {
    return __request(OpenAPI, {
      method: 'GET',
      url: '/v0/pipelines'
    })
  }

  /**
   * Create a new program configuration.
   * Create a new program configuration.
   * @param requestBody
   * @returns NewPipelineResponse Configuration successfully created.
   * @throws ApiError
   */
  public static newPipeline(requestBody: NewPipelineRequest): CancelablePromise<NewPipelineResponse> {
    return __request(OpenAPI, {
      method: 'POST',
      url: '/v0/pipelines',
      body: requestBody,
      mediaType: 'application/json',
      errors: {
        404: `Specified program id does not exist in the database.`
      }
    })
  }

  /**
   * Update existing program configuration.
   * Update existing program configuration.
   *
   * Updates program config name, description and code and, optionally, config
   * and connectors. On success, increments config version by 1.
   * @param requestBody
   * @returns UpdatePipelineResponse Configuration successfully updated.
   * @throws ApiError
   */
  public static updatePipeline(requestBody: UpdatePipelineRequest): CancelablePromise<UpdatePipelineResponse> {
    return __request(OpenAPI, {
      method: 'PATCH',
      url: '/v0/pipelines',
      body: requestBody,
      mediaType: 'application/json',
      errors: {
        404: `Specified connector id does not exist in the database.`
      }
    })
  }

  /**
   * Terminate and delete a pipeline.
   * Terminate and delete a pipeline.
   *
   * Shut down the pipeline if it is still running and delete it from
   * the database.
   * @param pipelineId Unique pipeline identifier
   * @returns string Pipeline successfully deleted.
   * @throws ApiError
   */
  public static pipelineDelete(pipelineId: string): CancelablePromise<string> {
    return __request(OpenAPI, {
      method: 'DELETE',
      url: '/v0/pipelines/{pipeline_id}',
      path: {
        pipeline_id: pipelineId
      },
      errors: {
        400: `Specified pipeline id is not a valid uuid.`,
        404: `Specified pipeline id does not exist in the database.`
      }
    })
  }

  /**
   * Return the last committed (and running, if pipeline is started)
   * Return the last committed (and running, if pipeline is started)
   * configuration of the pipeline.
   * @param pipelineId Unique pipeline identifier
   * @returns any Last committed configuration of the pipeline retrieved successfully (returns null if pipeline was never deployed yet).
   * @throws ApiError
   */
  public static pipelineCommitted(pipelineId: string): CancelablePromise<PipelineRevision | null> {
    return __request(OpenAPI, {
      method: 'GET',
      url: '/v0/pipelines/{pipeline_id}/committed',
      path: {
        pipeline_id: pipelineId
      },
      errors: {
        404: `Specified \`pipeline_id\` does not exist in the database.`
      }
    })
  }

  /**
   * Subscribe to a stream of updates to a SQL view or table.
   * Subscribe to a stream of updates to a SQL view or table.
   *
   * The pipeline responds with a continuous stream of changes to the specified
   * table or view, encoded using the format specified in the `?format=`
   * parameter. Updates are split into `Chunk`'s.
   *
   * The pipeline continuous sending updates until the client closes the
   * connection or the pipeline is shut down.
   * @param pipelineId Unique pipeline identifier.
   * @param tableName SQL table or view name.
   * @param format Output data format, e.g., 'csv' or 'json'.
   * @param query Query to execute on the table. Must be one of 'table', 'neighborhood', or 'quantiles'. The default value is 'table'
   * @param mode Output mode. Must be one of 'watch' or 'snapshot'. The default value is 'watch'
   * @param quantiles For 'quantiles' queries: the number of quantiles to output. The default value is 100.
   * @param requestBody When the `query` parameter is set to 'neighborhood', the body of the request must contain a neighborhood specification.
   * @returns Chunk Connection to the endpoint successfully established. The body of the response contains a stream of data chunks.
   * @throws ApiError
   */
  public static httpOutput(
    pipelineId: string,
    tableName: string,
    format: string,
    query?: OutputQuery | null,
    mode?: EgressMode | null,
    quantiles?: number | null,
    requestBody?: NeighborhoodQuery | null
  ): CancelablePromise<Chunk> {
    return __request(OpenAPI, {
      method: 'POST',
      url: '/v0/pipelines/{pipeline_id}/egress/{table_name}',
      path: {
        pipeline_id: pipelineId,
        table_name: tableName
      },
      query: {
        format: format,
        query: query,
        mode: mode,
        quantiles: quantiles
      },
      body: requestBody,
      mediaType: 'application/json',
      errors: {
        400: `Unknown data format specified in the '?format=' argument.`,
        404: `Specified table or view does not exist.`,
        410: `Pipeline is not currently running because it has been shutdown or not yet started.`,
        500: `Request failed.`
      }
    })
  }

  /**
   * Push data to a SQL table.
   * Push data to a SQL table.
   *
   * The client sends data encoded using the format specified in the `?format=`
   * parameter as a body of the request.  The contents of the data must match
   * the SQL table schema specified in `table_name`
   *
   * The pipeline ingests data as it arrives without waiting for the end of
   * the request.  Successful HTTP response indicates that all data has been
   * ingested successfully.
   * @param pipelineId Unique pipeline identifier.
   * @param tableName SQL table name.
   * @param format Input data format, e.g., 'csv' or 'json'.
   * @returns any Data successfully delivered to the pipeline.
   * @throws ApiError
   */
  public static httpInput(pipelineId: string, tableName: string, format: string): CancelablePromise<any> {
    return __request(OpenAPI, {
      method: 'POST',
      url: '/v0/pipelines/{pipeline_id}/ingress/{table_name}',
      path: {
        pipeline_id: pipelineId,
        table_name: tableName
      },
      query: {
        format: format
      },
      errors: {
        400: `Unknown data format specified in the '?format=' argument.`,
        404: `Pipeline is not currently running because it has been shutdown or not yet started.`,
        422: `Error parsing input data.`,
        500: `Request failed.`
      }
    })
  }

  /**
   * Retrieve pipeline metrics and performance counters.
   * Retrieve pipeline metrics and performance counters.
   * @param pipelineId Unique pipeline identifier
   * @returns any Pipeline metrics retrieved successfully.
   * @throws ApiError
   */
  public static pipelineStats(pipelineId: string): CancelablePromise<Record<string, any>> {
    return __request(OpenAPI, {
      method: 'GET',
      url: '/v0/pipelines/{pipeline_id}/stats',
      path: {
        pipeline_id: pipelineId
      },
      errors: {
        400: `Specified pipeline id is not a valid uuid.`,
        404: `Specified pipeline id does not exist in the database.`
      }
    })
  }

  /**
   * Validate the configuration of a  a pipeline.
   * Validate the configuration of a  a pipeline.
   *
   * Validate configuration, usable as a pre-cursor for deploy to
   * check if pipeline configuration is valid and can be deployed.
   * @param pipelineId Unique pipeline identifier
   * @returns string Validate a Pipeline config.
   * @throws ApiError
   */
  public static pipelineValidate(pipelineId: string): CancelablePromise<string> {
    return __request(OpenAPI, {
      method: 'GET',
      url: '/v0/pipelines/{pipeline_id}/validate',
      path: {
        pipeline_id: pipelineId
      },
      errors: {
        400: `The connectors in the config referenced a view that doesn't exist.`,
        404: `Specified pipeline id does not exist in the database.`,
        503: `Unable to start the pipeline before its program has been compiled.`
      }
    })
  }

  /**
   * Perform action on a pipeline.
   * Perform action on a pipeline.
   *
   * - 'deploy': Deploy a pipeline for the specified program and configuration.
   * This is a synchronous endpoint, which sends a response once the pipeline has
   * been initialized.
   * - 'start': Start a pipeline.
   * - 'pause': Pause the pipeline.
   * - 'shutdown': Terminate the execution of a pipeline. Sends a termination
   * request to the pipeline process. Returns immediately, without waiting for
   * the pipeline to terminate (which can take several seconds). The pipeline is
   * not deleted from the database, but its `status` is set to `shutdown`.
   * @param pipelineId Unique pipeline identifier
   * @param action Pipeline action [deploy, start, pause, shutdown]
   * @returns string Performed a Pipeline action.
   * @throws ApiError
   */
  public static pipelineAction(pipelineId: string, action: string): CancelablePromise<string> {
    return __request(OpenAPI, {
      method: 'POST',
      url: '/v0/pipelines/{pipeline_id}/{action}',
      path: {
        pipeline_id: pipelineId,
        action: action
      },
      errors: {
        400: `The connectors in the config referenced a view that doesn't exist.`,
        404: `Specified pipeline id does not exist in the database.`,
        500: `Timeout waiting for the pipeline to initialize. Indicates an internal system error.`,
        503: `Unable to start the pipeline before its program has been compiled.`
      }
    })
  }
}
