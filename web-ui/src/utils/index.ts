// This file contains some utility functions used throughout the project.

import { useState, useCallback } from 'react'

// Escape regular expressions
//
// See: https://stackoverflow.com/questions/3561493/is-there-a-regexp-escape-function-in-javascript
export const escapeRegExp = (value: string) => {
  return value.replace(/[-[\]{}()*+?.,\\^$|#\s]/g, '\\$&')
}

// Convert bytes (number of) to a human readable string
// e.g., 1024 returns "1 KiB"
export function humanSize(bytes: number): string {
  const thresh = 1024
  if (Math.abs(bytes) < thresh) {
    return bytes + ' B'
  }
  const units = ['KiB', 'MiB', 'GiB', 'TiB', 'PiB', 'EiB', 'ZiB', 'YiB']
  let u = -1
  do {
    bytes /= thresh
    ++u
  } while (Math.abs(bytes) >= thresh && u < units.length - 1)

  return bytes.toFixed(1) + ' ' + units[u]
}

// Check if a string is a valid URL using the Javascript URL class
//
// See: https://stackoverflow.com/questions/61634973/yup-validation-of-website-using-url-very-strict
export const isUrl = (url: string | undefined) => {
  if (!url) return false
  try {
    new URL(url)
  } catch (e) {
    return false
  }

  return true
}

// Generate a random name, used for giving random names in certain cases.
export const randomString = (): string => new Date().getTime().toString(36) + Math.random().toString(36).slice(2)

// Remove a prefix in a string (if it exists)
export const removePrefix = (value: string, prefix: string) =>
  value.startsWith(prefix) ? value.slice(prefix.length) : value

// Zips two lists together.
//
// console.log( zip([1,2,3], ["a","b","c","d"]) );
// [[1, "a"], [2, "b"], [3, "c"], [undefined, "d"]]
//
// See: https://stackoverflow.com/questions/22015684/zip-arrays-in-javascript
export const zip = (a: any, b: any) => Array.from(Array(Math.max(b.length, a.length)), (_, i) => [a[i], b[i]])

// Gives examples for the placeholder attributes in various form fields
// throughout the UI.
export const PLACEHOLDER_VALUES = {
  program_name: 'Average Price Calculator',
  program_description: 'Calculate the average price of a product',
  connector_name: 'House Price Data',
  connector_description: 'House price data from the UK Land Registry',
  pipeline_name: 'Price Checker',
  pipeline_description: 'Analyze e-commerce prices'
}

// A way to throw errors in async code so they can be catched with ErrorBoundary
// in react. Pretty silly, but it works.
//
// See:
// https://medium.com/trabe/catching-asynchronous-errors-in-react-using-error-boundaries-5e8a5fd7b971
export const useAsyncError = () => {
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const [_, setError] = useState()
  return useCallback(
    (e: any) => {
      setError(() => {
        throw e
      })
    },
    [setError]
  )
}
