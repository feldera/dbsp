// Hook that connects to the table/view updates and reads them line-by-line and
// then parses the lines.

import { Dispatch, MutableRefObject, SetStateAction, useCallback, useMemo } from 'react'
import { parse } from 'csv-parse'
import { GridApiPro } from '@mui/x-data-grid-pro/models/gridApiPro'
import { IntrospectionTableState } from '../IntrospectionTable'
import { NeighborhoodQuery } from 'src/types/manager'

// Read from a stream, yelding one line at a time.
//
// Adapted from:
// https://developer.mozilla.org/en-US/docs/Web/API/ReadableStreamDefaultReader/read#example_2_-_handling_text_line_by_line
async function* readLineFromStream(response: Response) {
  const utf8Decoder = new TextDecoder('utf-8')
  if (!response.body) {
    throw new Error('No body when fetching request.')
  }
  const reader = response.body.getReader()
  let { value: chunk, done: readerDone } = await reader.read()
  let decodedChunk = chunk ? utf8Decoder.decode(chunk, { stream: true }) : ''

  const re = /\r\n|\n|\r/gm
  let startIndex = 0

  for (;;) {
    const result = re.exec(decodedChunk)
    if (!result) {
      if (readerDone) {
        break
      }
      const remainder = decodedChunk.substring(startIndex)
      ;({ value: chunk, done: readerDone } = await reader.read())
      decodedChunk = remainder + (chunk ? utf8Decoder.decode(chunk, { stream: true }) : '')
      startIndex = re.lastIndex = 0
      continue
    }
    yield decodedChunk.substring(startIndex, result.index)
    startIndex = re.lastIndex
  }
  if (startIndex < decodedChunk.length) {
    // last line didn't end in a newline char
    yield decodedChunk.substring(startIndex)
  }
}

function useTableUpdater() {
  const utf8Decoder = useMemo(() => new TextDecoder('utf-8'), [])
  const readStream = useCallback(
    async (
      url: URL,
      neighborhood: NeighborhoodQuery,
      apiRef: MutableRefObject<GridApiPro>,
      tableState: IntrospectionTableState,
      controller: AbortController,
      setLoading: Dispatch<SetStateAction<boolean>>
    ) => {
      const response = await fetch(url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json'
        },
        body: JSON.stringify(neighborhood),
        signal: controller.signal
      }).catch(error => {
        return Promise.reject(error)
      })
      if (!response.ok) {
        if (!response.body) {
          throw new Error('Invalid error response from server: no body.')
        }
        const reader = response.body?.getReader()
        const { value: chunk } = await reader.read()
        const decodedChunk = chunk
          ? utf8Decoder.decode(chunk, { stream: false })
          : '{ "message": "Unable to decode server error: Invalid UTF-8 string.", "error_code": "UIInvalidUtf8" }'
        try {
          const error = JSON.parse(decodedChunk)
          return Promise.reject(error)
        } catch (e) {
          if (e instanceof SyntaxError) {
            throw new Error('Received invalid error format from server: ' + decodedChunk)
          } else {
            throw e
          }
        }
      }

      setLoading(false)
      for await (const line of readLineFromStream(response)) {
        const obj = JSON.parse(line)
        parse(
          obj.text_data,
          {
            delimiter: ','
          },
          (error, result: string[][]) => {
            if (error) {
              console.error(error)
            }

            const newRows: any[] = result.map(row => {
              const genId = row[0]
              const weight = row[row.length - 1]
              const fields = row.slice(1, row.length - 1)

              const newRow = { genId, weight: parseInt(weight) } as any
              tableState.headers.forEach((col, i) => {
                if (col.field !== 'genId' && col.field !== 'weight') {
                  newRow[col.field] = fields[i - 1]
                }
              })

              return newRow
            })

            apiRef.current?.updateRows(
              newRows
                .map(row => {
                  const curRow = apiRef.current.getRow(row.genId)
                  if (curRow !== null && curRow.weight + row.weight == 0) {
                    return row
                  } else if (curRow == null && row.weight < 0) {
                    return null
                  } else {
                    return { ...row, weight: row.weight + (curRow?.weight || 0) }
                  }
                })
                .filter(x => x !== null)
            )
          }
        )
      }
    },
    [utf8Decoder]
  )

  return readStream
}

export default useTableUpdater
