<script lang="ts">
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Tooltip from '$lib/components/ui/tooltip/index.js';
	import InfoIcon from '@lucide/svelte/icons/info';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import XIcon from '@lucide/svelte/icons/x';
	import CheckIcon from '@lucide/svelte/icons/check';
	import BookmarkPlusIcon from '@lucide/svelte/icons/bookmark-plus';
	import { toast } from 'svelte-sonner';
	import { onMount } from 'svelte';
	import { cn } from '$lib/utils.js';

	interface Props {
		id?: string;
		value?: string;
		placeholder?: string;
		label?: string;
		description?: string;
		showTooltip?: boolean;
		tooltipContent?: string;
		allowEmpty?: boolean;
		disabled?: boolean;
		class?: string;
	}

	let {
		id = 'video-name',
		value = $bindable(''),
		placeholder = '{{title}}',
		label = '视频文件名模板',
		description = '',
		showTooltip = true,
		tooltipContent = '',
		allowEmpty = true,
		disabled = false,
		class: className = ''
	}: Props = $props();

	const STORAGE_KEY = 'bili_sync_custom_filename_templates';

	const PRESET_TEMPLATES = [
		{ template: '{{title}}', label: '默认标题' },
		{ template: '{{pubtime}} {{title}}', label: '发布时间 标题' },
		{ template: '{{upper_name}} - {{title}}', label: 'UP主 - 标题' },
		{ template: '{{pubtime}}_{{upper_name}}_{{title}}', label: '时间_UP主_标题' },
		{ template: '{{bvid}} - {{title}}', label: 'BV号 - 标题' },
		{ template: '[{{upper_name}}] {{title}}', label: '[UP主] 标题' }
	];

	const VARIABLES = [
		{ name: 'title', label: '视频标题' },
		{ name: 'pubtime', label: '发布时间' },
		{ name: 'upper_name', label: 'UP主名称' },
		{ name: 'upper_mid', label: 'UP主ID' },
		{ name: 'bvid', label: 'BV号' },
		{ name: 'fav_time', label: '收藏时间' }
	];

	let customTemplates = $state<string[]>([]);
	let isAdding = $state(false);
	let newTemplateInput = $state('');
	let mainInputRef = $state<HTMLInputElement | null>(null);
	let addInputRef = $state<HTMLInputElement | null>(null);

	function loadCustomTemplates() {
		if (typeof window === 'undefined') return;
		try {
			const saved = localStorage.getItem(STORAGE_KEY);
			if (saved) {
				const parsed = JSON.parse(saved);
				if (Array.isArray(parsed)) {
					customTemplates = parsed.filter((t) => typeof t === 'string' && t.trim().length > 0);
				}
			}
		} catch (e) {
			console.error('Failed to load custom filename templates:', e);
		}
	}

	function saveCustomTemplates(templates: string[]) {
		customTemplates = templates;
		if (typeof window === 'undefined') return;
		try {
			localStorage.setItem(STORAGE_KEY, JSON.stringify(templates));
		} catch (e) {
			console.error('Failed to save custom filename templates:', e);
		}
	}

	function handleSelect(t: string) {
		if (value === t && allowEmpty) {
			value = '';
		} else {
			value = t;
		}
	}

	function handleAddCustom(strToAdd?: string) {
		const target = (strToAdd ?? newTemplateInput).trim();
		if (!target) {
			toast.error('模板内容不能为空');
			return;
		}

		if (PRESET_TEMPLATES.some((p) => p.template === target)) {
			toast.info('该模板已在常用预设模板中');
			value = target;
			isAdding = false;
			newTemplateInput = '';
			return;
		}

		if (customTemplates.includes(target)) {
			toast.info('该自定义模板已存在');
			value = target;
			isAdding = false;
			newTemplateInput = '';
			return;
		}

		saveCustomTemplates([...customTemplates, target]);
		value = target;
		isAdding = false;
		newTemplateInput = '';
		toast.success('已保存自定义模板');
	}

	function handleDeleteCustom(t: string) {
		saveCustomTemplates(customTemplates.filter((item) => item !== t));
		toast.success('已移除自定义模板');
	}

	function insertVariable(varName: string) {
		const token = `{{${varName}}}`;
		if (isAdding && addInputRef) {
			const start = addInputRef.selectionStart ?? newTemplateInput.length;
			const end = addInputRef.selectionEnd ?? newTemplateInput.length;
			newTemplateInput = newTemplateInput.slice(0, start) + token + newTemplateInput.slice(end);
			const nextPos = start + token.length;
			setTimeout(() => {
				addInputRef?.focus();
				addInputRef?.setSelectionRange(nextPos, nextPos);
			}, 0);
		} else if (mainInputRef) {
			const start = mainInputRef.selectionStart ?? value.length;
			const end = mainInputRef.selectionEnd ?? value.length;
			value = value.slice(0, start) + token + value.slice(end);
			const nextPos = start + token.length;
			setTimeout(() => {
				mainInputRef?.focus();
				mainInputRef?.setSelectionRange(nextPos, nextPos);
			}, 0);
		} else {
			value += token;
		}
	}

	let canSaveCurrentAsCustom = $derived(
		value.trim().length > 0 &&
			!PRESET_TEMPLATES.some((p) => p.template === value.trim()) &&
			!customTemplates.includes(value.trim())
	);

	$effect(() => {
		if (isAdding && addInputRef) {
			addInputRef.focus();
		}
	});

	onMount(() => {
		loadCustomTemplates();
		const handleStorage = (e: StorageEvent) => {
			if (e.key === STORAGE_KEY) {
				loadCustomTemplates();
			}
		};
		window.addEventListener('storage', handleStorage);
		return () => {
			window.removeEventListener('storage', handleStorage);
		};
	});
</script>

<div class={cn('space-y-2', className)}>
	<!-- 标题与操作栏 -->
	<div class="flex items-center justify-between">
		<div class="flex items-center space-x-2">
			{#if label}
				<Label for={id} class="text-sm font-medium">{label}</Label>
			{/if}
			{#if showTooltip}
				<Tooltip.Root>
					<Tooltip.Trigger>
						<InfoIcon class="text-muted-foreground h-3.5 w-3.5" />
					</Tooltip.Trigger>
					<Tooltip.Content>
						<p class="text-xs">
							{tooltipContent ||
								(placeholder
									? `留空则使用全局默认模板（当前为：${placeholder}）。`
									: '留空则使用全局默认模板。')}
						</p>
					</Tooltip.Content>
				</Tooltip.Root>
			{/if}
		</div>
		{#if canSaveCurrentAsCustom}
			<button
				type="button"
				onclick={() => handleAddCustom(value)}
				class="text-primary hover:text-primary/80 flex cursor-pointer items-center gap-1 text-xs font-medium transition-colors"
				title="将当前输入的内容保存为常用模板"
			>
				<BookmarkPlusIcon class="h-3.5 w-3.5" />
				<span>存为常用</span>
			</button>
		{/if}
	</div>

	<!-- 文本输入框 -->
	<div class="relative">
		<Input
			{id}
			bind:ref={mainInputRef}
			type="text"
			bind:value
			{placeholder}
			{disabled}
			class="font-mono text-sm"
		/>
	</div>

	<!-- 常用模板与维护操作 -->
	<div class="space-y-1.5 pt-0.5">
		<div class="text-muted-foreground flex items-center justify-between text-xs">
			<span class="font-medium">常用模板（点选即可使用）：</span>
			<div class="flex items-center gap-2">
				{#if !isAdding}
					<button
						type="button"
						onclick={() => {
							isAdding = true;
							newTemplateInput = '';
						}}
						class="text-primary flex cursor-pointer items-center gap-0.5 font-medium hover:underline"
					>
						<PlusIcon class="h-3 w-3" />
						<span>新增模板</span>
					</button>
				{/if}
			</div>
		</div>

		<!-- 模板列表 -->
		<div class="flex flex-wrap gap-1.5">
			{#each PRESET_TEMPLATES as item (item.template)}
				{@const active = value === item.template}
				<button
					type="button"
					onclick={() => handleSelect(item.template)}
					class={cn(
						'inline-flex cursor-pointer items-center gap-1 rounded-md border px-2 py-1 font-mono text-xs transition-colors select-none',
						active
							? 'bg-primary text-primary-foreground border-primary font-medium shadow-xs'
							: 'bg-secondary/60 hover:bg-secondary text-secondary-foreground hover:border-border border-transparent'
					)}
					title={item.label ? `${item.label} (再次点击清空)` : '点击使用，再次点击清空'}
				>
					{#if active}
						<CheckIcon class="h-3 w-3" />
					{/if}
					<span>{item.template}</span>
				</button>
			{/each}

			{#each customTemplates as customTpl (customTpl)}
				{@const active = value === customTpl}
				<div
					class={cn(
						'group inline-flex items-center gap-1 rounded-md border py-1 pr-1 pl-2 font-mono text-xs transition-colors select-none',
						active
							? 'bg-primary text-primary-foreground border-primary font-medium shadow-xs'
							: 'bg-secondary/60 hover:bg-secondary text-secondary-foreground hover:border-border border-transparent'
					)}
				>
					<button
						type="button"
						onclick={() => handleSelect(customTpl)}
						class="inline-flex cursor-pointer items-center gap-1"
						title="自定义模板（点击使用，再次点击清空）"
					>
						{#if active}
							<CheckIcon class="h-3 w-3" />
						{/if}
						<span class="max-w-[200px] truncate sm:max-w-none">{customTpl}</span>
					</button>
					<button
						type="button"
						onclick={(e) => {
							e.stopPropagation();
							handleDeleteCustom(customTpl);
						}}
						class={cn(
							'hover:bg-destructive/20 hover:text-destructive cursor-pointer rounded-xs p-0.5 transition-colors',
							active ? 'text-primary-foreground/80 hover:text-white' : 'text-muted-foreground'
						)}
						title="删除该自定义模板"
					>
						<XIcon class="h-3 w-3" />
					</button>
				</div>
			{/each}
		</div>

		<!-- 手动维护新增模板 (Inline) -->
		{#if isAdding}
			<div class="border-border bg-muted/40 mt-2 space-y-2 rounded-md border p-2.5">
				<div class="flex items-center justify-between text-xs font-medium">
					<span>手动维护新增模板</span>
					<button
						type="button"
						onclick={() => (isAdding = false)}
						class="text-muted-foreground hover:text-foreground cursor-pointer"
					>
						<XIcon class="h-3.5 w-3.5" />
					</button>
				</div>
				<div class="flex items-center gap-1.5">
					<Input
						bind:ref={addInputRef}
						type="text"
						bind:value={newTemplateInput}
						placeholder="例如：&#123;&#123;upper_name&#125;&#125;/&#123;&#123;title&#125;&#125;"
						class="h-8 flex-1 font-mono text-xs"
						onkeydown={(e) => {
							if (e.key === 'Enter') {
								e.preventDefault();
								handleAddCustom();
							} else if (e.key === 'Escape') {
								isAdding = false;
							}
						}}
					/>
					<Button size="sm" class="h-8 px-2.5 text-xs" onclick={() => handleAddCustom()}>
						保存
					</Button>
					<Button
						variant="outline"
						size="sm"
						class="h-8 px-2.5 text-xs"
						onclick={() => (isAdding = false)}
					>
						取消
					</Button>
				</div>
				<div class="text-muted-foreground flex flex-wrap items-center gap-1 text-[11px]">
					<span>点击插入变量：</span>
					{#each VARIABLES as v}
						<button
							type="button"
							onclick={() => insertVariable(v.name)}
							class="bg-background hover:bg-accent hover:text-accent-foreground cursor-pointer rounded border px-1 py-0.5 font-mono transition-colors"
							title={v.label}
						>
							+{v.name}
						</button>
					{/each}
				</div>
			</div>
		{/if}

		<!-- 变量参考与提示说明 -->
		<div class="text-muted-foreground flex items-center justify-between pt-0.5 text-xs">
			<p>{description || '留空则使用全局默认模板。支持 Handlebars 模板语法。'}</p>
			{#if !isAdding}
				<div class="hidden items-center gap-1 text-[11px] sm:flex">
					<span>可用变量：</span>
					{#each VARIABLES as v}
						<button
							type="button"
							onclick={() => insertVariable(v.name)}
							class="hover:text-primary cursor-pointer font-mono hover:underline"
							title={`点击插入 ${v.name} (${v.label})`}
						>
							{v.name}
						</button>
					{/each}
				</div>
			{/if}
		</div>
	</div>
</div>
