import type { AuthoredTemplate } from "./AuthoredTemplate";
export interface TemplatesListAuthoredResponse {
    templates: Array<AuthoredTemplate>;
    total_pages: number;
}
