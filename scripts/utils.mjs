import { styleText } from 'node:util'

export const log_success = (msg, ...optionalParams) =>
  console.log(styleText('green', msg), ...optionalParams)
export const log_error = (msg, ...optionalParams) =>
  console.log(styleText('red', msg), ...optionalParams)
export const log_info = (msg, ...optionalParams) =>
  console.log(styleText('bgBlue', msg), ...optionalParams)
export const log_debug = (msg, ...optionalParams) =>
  console.log(styleText('gray', msg), ...optionalParams)
