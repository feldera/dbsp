// Hook that connects to the table/view updates and reads them line-by-line and
// then parses the lines.

import { MutableRefObject, useCallback } from 'react'
import { parse } from 'csv-parse'
import { GridApiPro } from '@mui/x-data-grid-pro/models/gridApiPro'

// Read from a stream, yelding one line at a time.
//
// Adapted from:
// https://developer.mozilla.org/en-US/docs/Web/API/ReadableStreamDefaultReader/read#example_2_-_handling_text_line_by_line
async function* readLineFromStream(url: URL) {
  const response = await fetch(url)
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
  const readStream = useCallback(
    async (url: URL, apiRef: MutableRefObject<GridApiPro>, headers: { field: string }[]) => {
      for await (const line of readLineFromStream(url)) {
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
              const fields = row.slice(0, row.length - 1)

              const newRow = { genId, weight: parseInt(weight) } as any
              headers.forEach((col, i) => {
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
    []
  )

  return readStream
}

export default useTableUpdater
