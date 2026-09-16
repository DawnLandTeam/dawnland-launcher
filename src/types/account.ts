export interface Account {
  id: string;
  username: string;
  accountType: "offline" | "microsoft" | "authlib";
  accessToken?: string;
  refreshToken?: string;
  textures?: AccountTextures;
  authlibUrl?: string;
  authlibServerName?: string;
  authlibEmail?: string;
}

export interface AccountTextures {
  skinUrl?: string;
  capeUrl?: string;
  variant?: string;
}

export interface AuthlibAuthResult {
  accessToken: string;
  clientToken: string;
  availableProfiles: any[] | null | undefined;
  authlibServerName?: string;
}

export interface LoginInitResponse {
  userCode: string;
  deviceCode: string;
  verificationUri: string;
  message: string;
}
