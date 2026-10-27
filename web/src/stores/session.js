import { create } from 'zustand'

/** 当前登录用户（来源为 `/api/auth/me`，仅缓存，不作为安全边界）。 */
export const useSessionStore = create((set) => ({
  user: null,
  setUser: (user) => set({ user }),
  clear: () => set({ user: null }),
}))
