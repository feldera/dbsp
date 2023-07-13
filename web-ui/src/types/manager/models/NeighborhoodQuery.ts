/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */

/**
 * A request to output a specific neighborhood of a table or view.
 * The neighborhood is defined in terms of its central point (`anchor`)
 * and the number of rows preceding and following the anchor to output.
 */
export type NeighborhoodQuery = {
  after: number
  anchor?: Record<string, any> | null
  before: number
}
