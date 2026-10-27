const BASE = '/api'

export class ApiError extends Error {
  constructor(status, code, message) {
    super(message || code || `HTTP ${status}`)
    this.name = 'ApiError'
    this.status = status
    this.code = code
  }
}

/**
 * 统一 `/api` 请求：携带 Cookie、JSON 编解码、解析统一错误体。
 * 失败抛出 `ApiError { status, code, message }`。
 */
export async function request(path, { method = 'GET', body, headers, ...rest } = {}) {
  let res
  try {
    res = await fetch(`${BASE}${path}`, {
      method,
      credentials: 'same-origin',
      headers: {
        ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}),
        ...headers,
      },
      body: body !== undefined ? JSON.stringify(body) : undefined,
      ...rest,
    })
  } catch {
    throw new ApiError(0, 'network', 'network error')
  }

  if (res.status === 204) return null

  const text = await res.text()
  let data = null
  if (text) {
    try {
      data = JSON.parse(text)
    } catch {
      data = null
    }
  }

  if (!res.ok) {
    const error = data?.error
    throw new ApiError(res.status, error?.code, error?.message)
  }
  return data
}
