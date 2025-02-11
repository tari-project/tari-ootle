import {create} from "zustand";
import {persist} from "zustand/middleware";

interface Store {
  authToken: string;
  setAuthToken: (token: string) => void;
}

const useAuthStore = create<Store>()(
  persist<Store>(
    (set) => ({
        authToken: "",
        setAuthToken: (token) => set({authToken: token}),
    }),
    {
      name: "tari-auth",
    },
  ),
);

export default useAuthStore;