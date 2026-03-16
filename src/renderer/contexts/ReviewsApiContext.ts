import { createContext, useContext } from 'react'
import type { ReviewsApi } from '../types'

export const ReviewsApiContext = createContext<ReviewsApi | null>(null)

export function useReviewsApi(): ReviewsApi {
  const api = useContext(ReviewsApiContext)
  if (!api) {
    throw new Error('useReviewsApi must be used within a ReviewsApiContext.Provider')
  }
  return api
}
