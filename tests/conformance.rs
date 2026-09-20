//! The reference assertion suite, ported from the C++ implementation's tests.
//!
//! Every case is checked and all failures are reported together, which makes a
//! behavioural regression easy to read even when it affects many inputs.

// The suite mirrors the reference tests statement for statement, including the
// way it builds options by mutating a default in sequence.
#![allow(clippy::too_many_lines, clippy::field_reassign_with_default)]

use normalize_uk::uktextnorm::{
    expand_abbreviations, flag_uncertain, normalize, normalize_abbreviations, normalize_preset,
    normalize_with, number_to_ordinal_words, number_to_words, number_to_words_case,
    transliterate_to_cyrillic, ColonStyle, CurrencySymbolPolicy, DateStyle, GrammaticalCase,
    NormalizeOptions, NormalizePreset, NumericDateOrder, OrdinalForm, PhoneStyle, QuoteStyle,
    RangeStyle, SymbolStyle, UncertainSpan, UncertaintyCategory, UncertaintySeverity,
};
use std::cell::RefCell;

thread_local! {
    /// Failures collected while the suite runs.
    static FAILURES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn fail(message: String) {
    FAILURES.with(|f| f.borrow_mut().push(message));
}

fn check(name: &str, actual: impl AsRef<str>, expected: impl AsRef<str>) {
    let (actual, expected) = (actual.as_ref(), expected.as_ref());
    if actual != expected {
        fail(format!("{name}\n  expected: {expected}\n  actual:   {actual}"));
    }
}

fn check_absent(name: &str, actual: impl AsRef<str>, unexpected: &str) {
    let actual = actual.as_ref();
    if actual.contains(unexpected) {
        fail(format!("{name}\n  unexpected fragment: {unexpected}\n  actual: {actual}"));
    }
}

/// Renders the spans for a failure message.
fn describe(spans: &[UncertainSpan]) -> String {
    spans
        .iter()
        .map(|s| format!("  {}, {}, {}, {}", s.start, s.stop, s.text, s.reason))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Checks the span at `index` by its text, and that its offsets agree with it.
fn check_span_at(name: &str, spans: &[UncertainSpan], index: usize, text: &str) {
    match spans.get(index) {
        Some(span) if span.text == text => {}
        Some(span) => {
            fail(format!("{name}\n  expected span {index}: {text}\n  actual: {}", span.text));
        }
        None => fail(format!("{name}\n  missing span {index}: {text}")),
    }
}

fn check_span_reason(name: &str, spans: &[UncertainSpan], text: &str, reason_fragment: &str) {
    if spans.iter().any(|s| s.text == text && s.reason.contains(reason_fragment)) {
        return;
    }
    fail(format!(
        "{name}\n  expected span containing: {text} / {reason_fragment}\n  actual spans:\n{}",
        describe(spans)
    ));
}

fn check_span_meta(
    name: &str,
    spans: &[UncertainSpan],
    text: &str,
    category: UncertaintyCategory,
    severity: UncertaintySeverity,
) {
    if spans.iter().any(|s| s.text == text && s.category == category && s.severity == severity) {
        return;
    }
    fail(format!(
        "{name}\n  expected metadata span: {text} ({category:?}, {severity:?})\n  actual spans:\n{}",
        describe(spans)
    ));
}

fn check_no_span_meta(
    name: &str,
    spans: &[UncertainSpan],
    text: &str,
    severity: UncertaintySeverity,
) {
    if spans.iter().any(|s| s.text == text && s.severity == severity) {
        fail(format!("{name}\n  unexpected metadata span: {text}"));
    }
}

fn check_no_category(name: &str, spans: &[UncertainSpan], category: UncertaintyCategory) {
    if let Some(span) = spans.iter().find(|s| s.category == category) {
        fail(format!("{name}\n  unexpected {category:?} span: {}", span.text));
    }
}

#[test]
fn reference_assertions() {
    run();
    let failures = FAILURES.with(|f| f.borrow().clone());
    assert!(
        failures.is_empty(),
        "{} of the reference assertions failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn run() {
    check(
        "number",
        number_to_words(1_234_567),
        "один мільйон двісті тридцять чотири тисячі п'ятсот шістдесят сім",
    );
    check("ordinal", number_to_ordinal_words(21, OrdinalForm::NomF), "двадцять перша");
    check("ordinal ins", number_to_ordinal_words(5, OrdinalForm::Ins), "п'ятим");
    check("ordinal ins soft", number_to_ordinal_words(3, OrdinalForm::Ins), "третім");
    check("ordinal ins fem", number_to_ordinal_words(2, OrdinalForm::InsF), "другою");
    check("ordinal loc fem", number_to_ordinal_words(40, OrdinalForm::LocF), "сороковій");
    check("ordinal ins plural", number_to_ordinal_words(100, OrdinalForm::InsPl), "сотими");
    check("ordinal loc", number_to_ordinal_words(1000, OrdinalForm::Loc), "тисячному");
    check("case", number_to_words_case(500, GrammaticalCase::Genitive), "п'ятисот");
    check("abbr", normalize_abbreviations("І т. д. і т. ін."), "І так далі і таке інше.");
    check("acronym", expand_abbreviations("СБР і НАТО"), "ес бе ер і НАТО");
    check("transliterate to cyrillic", transliterate_to_cyrillic("Google shop"), "гугле шоп");
    check(
        "cyrilize alias",
        transliterate_to_cyrillic("Google shop"),
        transliterate_to_cyrillic("Google shop"),
    );
    check("date", normalize("01.05.2024"), "перше травня дві тисячі двадцять четвертого року");
    check(
        "textual date consumes explicit year word",
        normalize("Подію завершили 1 вересня 1969 року."),
        "Подію завершили першого вересня тисяча дев'ятсот шістдесят дев'ятого року.",
    );
    check(
        "named month year consumes explicit year word",
        normalize("Дані за березень 2009 року."),
        "Дані за березень дві тисячі дев'ятого року.",
    );
    check(
        "genitive named month year",
        normalize("За даними березня 2009 року."),
        "За даними березня дві тисячі дев'ятого року.",
    );
    check(
        "instrumental year",
        normalize("Порівняно із 2018 роком."),
        "Порівняно із дві тисячі вісімнадцятим роком.",
    );
    check(
        "contextual abbreviated year",
        normalize("У 1993 р. оприлюднили звіт."),
        "У тисяча дев'ятсот дев'яносто третьому році оприлюднили звіт.",
    );
    check(
        "locative month and year",
        normalize("Подію провели в березні 2001."),
        "Подію провели в березні дві тисячі першого року.",
    );
    check(
        "decade without written suffix",
        normalize("У 1940 роках створили перші системи."),
        "У тисяча дев'ятсот сорокових роках створили перші системи.",
    );
    check("time", normalize("Зустріч о 06:06"), "Зустріч о шостій годині шість хвилин");
    check("currency", normalize("Ціна 12.50 грн"), "Ціна дванадцять гривень п'ятдесят копійок");
    check("measure", normalize("5 кг і 2 хв"), "п'ять кілограмів і дві хвилини");
    check(
        "latin measurement symbols",
        normalize("5 cm і 2 MHz"),
        "п'ять сантиметрів і два мегагерци",
    );
    check("micro-unit symbols", normalize("3 μL і 2 µg"), "три мікролітри і два мікрограми");
    check(
        "new counted nouns",
        normalize("2 книги і 21 сторінка"),
        "дві книги і двадцять одна сторінка",
    );
    check(
        "broader counted nouns",
        normalize("2 програми і 5 повідомлень"),
        "дві програми і п'ять повідомлень",
    );
    check(
        "locative noun forms",
        normalize("У 3 програмах, на 7 сторінках та з 5 документами"),
        "У трьох програмах, на семи сторінках та з п'ятьма документами",
    );
    check("article count outside legal labels", normalize("2 статті"), "дві статті");
    check(
        "mixed fraction before counted noun",
        normalize("2 3/4 книги"),
        "дві і три четвертих книги",
    );
    check(
        "expanded brand and technical readings",
        normalize("Adobe, Firefox, browser та plugin"),
        "адобі, фаєрфокс, браузер та плагін",
    );
    check(
        "additional brand and technical readings",
        normalize("Figma, Viber, Diia і compiler"),
        "фігма, вайбер, дія і компайлер",
    );
    check(
        "additional rate units",
        normalize("3 KB/s і 5 µg/m³"),
        "три кілобайти за секунду і п'ять мікрограмів на кубічний метр",
    );
    check(
        "expanded acronym readings",
        normalize("ШІ та ООН"),
        "Штучний інтелект та організація об'єднаних націй",
    );
    check("capitalized abbreviation expansion", normalize("Див. табл. 2"), "Дивись таблиця два");
    check("web", normalize("test@example.com"), "тест равлик ексампле крапка ком");
    check(
        "mixed",
        normalize("Python 3.11, GPS, 50%"),
        "пайтон три крапка одинадцять, джі пі ес, п'ятдесят відсотків",
    );
    check(
        "phone",
        normalize("+380 67 123-45-67"),
        "плюс триста вісімдесят шістдесят сім сто двадцять три сорок п'ять шістдесят сім",
    );
    check(
        "local phone",
        normalize("067-123-45-67"),
        "плюс триста вісімдесят шістдесят сім сто двадцять три сорок п'ять шістдесят сім",
    );
    check(
        "address",
        normalize("м. Київ, вул. Хрещатик, буд. 1, кв. 7"),
        "місто Київ, вулиця Хрещатик, будинок один, квартира сім",
    );
    check(
        "city abbreviation after locative preposition",
        normalize("Офіс у м. Києві."),
        "Офіс у місті Києві.",
    );
    check(
        "city abbreviation after genitive preposition",
        normalize("Дуга простягається від м. Гаммерфест."),
        "Дуга простягається від міста Гаммерфест.",
    );
    check(
        "unambiguous genitive city abbreviation",
        normalize("На 15 км проспекту м. Києва."),
        "На п'ятнадцять кілометрів проспекту міста Києва.",
    );
    check(
        "ordinal with full neuter suffix",
        normalize("Тернопіль займає 1-ше місце."),
        "Тернопіль займає перше місце.",
    );
    check(
        "ordinal with full second and third suffixes",
        normalize("2-ге і 3-тє місця."),
        "друге і третє місця.",
    );
    check(
        "year range",
        normalize("2020-2024 рр."),
        "дві тисячі двадцятий дві тисячі двадцять четвертий роки.",
    );
    check("case year", normalize("у 2024 році"), "у дві тисячі двадцять четвертому році");
    check("ordinal suffix", normalize("1991-го"), "тисяча дев'ятсот дев'яносто першого");
    check("roman century", normalize("XXI ст."), "двадцять перше століття");
    check("unit range", normalize("5-7 кг"), "п'ять сім кілограмів");
    check("percent range", normalize("10-15%"), "десять п'ятнадцять відсотків");
    check("multiplier", normalize("2 млн користувачів"), "два мільйони користувачів");
    check("multiplier currency", normalize("2 млн грн"), "два мільйони гривень");
    check("symbol multiplier currency", normalize("$3 млн"), "три мільйони доларів");
    check(
        "known acronym",
        normalize("ФОП і ПДВ"),
        "Фізична особа підприємець і податок на додану вартість",
    );
    check(
        "known acronym sentence casing",
        normalize("КМУ ухвалив. ПДВ 20%"),
        "Кабінет міністрів України ухвалив. Податок на додану вартість двадцять відсотків",
    );
    check("known acronym mid sentence casing", normalize("Постанова КМУ № 123/2026-р"), "Постанова кабінет міністрів України номер сто двадцять три слеш дві тисячі двадцять шість дефіс ер");
    check("institution acronyms", normalize("МОН, МОЗ і НБУ"), "Міністерство освіти і науки України, міністерство охорони здоров'я України і національний банк України");
    check("law enforcement acronyms", normalize("НАБУ, САП, ДБР і СБУ"), "Національне антикорупційне бюро України, спеціалізована антикорупційна прокуратура, державне бюро розслідувань і служба безпеки України");
    check("business admin acronyms", normalize("ТОВ, АТ, ОСББ, ОВА і РДА"), "Товариство з обмеженою відповідальністю, акціонерне товариство, об'єднання співвласників багатоквартирного будинку, обласна військова адміністрація і районна державна адміністрація");
    check("preposition genitive", normalize("до 5 кг"), "до п'яти кілограмів");
    check("instrumental context", normalize("з 3 друзями"), "з трьома друзями");
    check("prepositional oblique", normalize("у 4 містах"), "у чотирьох містах");
    check("ambiguous preposition with quantity", normalize("у 100 разів"), "у сто разів");
    check("accusative quantity after na", normalize("на 49 кубітів"), "на сорок дев'ять кубітів");
    check(
        "genitive quantity after sered",
        normalize("серед 42 творів"),
        "серед сорока двох творів",
    );
    check(
        "genitive quantity after z",
        normalize("приблизно з 50 кубітів"),
        "приблизно з п'ятдесяти кубітів",
    );
    check(
        "comparative phrase after governor",
        normalize("після більш ніж 5 років"),
        "після більш ніж п'яти років",
    );
    check("quantity after ponad", normalize("понад 1200 кубітів"), "понад тисячу двісті кубітів");
    check("counted masculine noun", normalize("21 користувач"), "двадцять один користувач");
    check("counted feminine noun", normalize("22 заявки"), "двадцять дві заявки");
    check("counted neuter noun", normalize("21 місто"), "двадцять одне місто");
    check("counted irregular person", normalize("5 людей"), "п'ять людей");
    check("counted irregular child", normalize("12 дітей"), "дванадцять дітей");
    check("counted document plural", normalize("104 документи"), "сто чотири документи");
    check("counted noun after ponad", normalize("понад 21 місто"), "понад двадцять одне місто");
    check("counted noun genitive governor", normalize("до 5 осіб"), "до п'яти осіб");
    check(
        "counted noun approximate governor",
        normalize("близько 12 дітей"),
        "близько дванадцяти дітей",
    );
    check("ordinal class", normalize("3 клас"), "третій клас");
    check("ordinal place", normalize("2 місце"), "друге місце");
    check("compound adjective", normalize("5-річний план"), "п'ятирічний план");
    check(
        "number groups",
        normalize("1 234 567 грн"),
        "один мільйон двісті тридцять чотири тисячі п'ятсот шістдесят сім гривень",
    );
    check(
        "legal sections",
        normalize("ч. 2 ст. 19, п. 3 розд. II"),
        "частина два стаття дев'ятнадцять, пункт три розділ другий",
    );
    check("century not section", normalize("XXI ст."), "двадцять перше століття");
    check(
        "slash date",
        normalize("15/06/2026"),
        "п'ятнадцяте червня дві тисячі двадцять шостого року",
    );
    check(
        "iso date",
        normalize("2026-06-15"),
        "п'ятнадцяте червня дві тисячі двадцять шостого року",
    );
    check(
        "abbrev month",
        normalize("15 черв. 2026"),
        "п'ятнадцятого червня дві тисячі двадцять шостого року",
    );
    check(
        "day month date range",
        normalize("15-16 червня 2026"),
        "п'ятнадцятого шістнадцятого червня дві тисячі двадцять шостого року",
    );
    check("numeric date range", normalize("15.06.2026-16.06.2026"), "п'ятнадцяте червня дві тисячі двадцять шостого року шістнадцяте червня дві тисячі двадцять шостого року");
    check("roman century range", normalize("XIX-XX ст."), "дев'ятнадцяте двадцяте століття");
    check("roman section range", normalize("I-IV розд."), "перший четвертий розділ");
    check(
        "roman quarter",
        normalize("II кв. 2026"),
        "другий квартал дві тисячі двадцять шостого року",
    );
    check("numeric quarter", normalize("2-й кв."), "другий квартал");
    check("apartment still address", normalize("кв. 7"), "квартира сім");
    check("marked hour", normalize("о 6-й"), "о шоста година");
    check("time part", normalize("6:00 ранку"), "шість годин ранку");
    check(
        "time with seconds",
        normalize("Зустріч о 12:34:56"),
        "Зустріч о дванадцятій годині тридцять чотири хвилини п'ятдесят шість секунд",
    );
    check(
        "comma currency",
        normalize("1 234,56 грн"),
        "тисяча двісті тридцять чотири гривні п'ятдесят шість копійок",
    );
    check(
        "symbol prefix currency",
        normalize("₴1234.56"),
        "тисяча двісті тридцять чотири гривні п'ятдесят шість копійок",
    );
    check("dot decimal measure", normalize("2.5 кг"), "дві цілих і п'ять десятих кілограма");
    check("dot decimal percent", normalize("12.5%"), "дванадцять цілих і п'ять десятих відсотка");
    check(
        "genitive governed percent",
        normalize("Зростання становить близько 67 %."),
        "Зростання становить близько шістдесяти семи відсотків.",
    );
    check(
        "accusative percent increase",
        normalize("Показник зріс на 33 %."),
        "Показник зріс на тридцять три відсотки.",
    );
    check(
        "genitive percent upper bound",
        normalize("Частка піднялася до 82 %."),
        "Частка піднялася до вісімдесяти двох відсотків.",
    );
    check(
        "dot decimal multiplier currency",
        normalize("1.5 млн грн"),
        "одна ціла і п'ять десятих мільйона гривень",
    );
    check(
        "governed multiplier",
        normalize("близько 327 млн користувачів"),
        "близько трьохсот двадцяти семи мільйонів користувачів",
    );
    check(
        "terminal multiplier punctuation",
        normalize("Користувачів 5 млн."),
        "Користувачів п'ять мільйонів.",
    );
    check("iban", normalize("UA213223130000026007233566001"), "айбан ю ей два один три два два три один три нуль нуль нуль нуль нуль два шість нуль нуль сім два три три п'ять шість шість нуль нуль один");
    check("edrpou", normalize("ЄДРПОУ 12345678"), "єдиний державний реєстр підприємств та організацій України один два три чотири п'ять шість сім вісім");
    check("postcode", normalize("індекс 01001"), "індекс нуль один нуль нуль один");
    check(
        "tax id",
        normalize("РНОКПП 1234567890"),
        "рнокпп один два три чотири п'ять шість сім вісім дев'ять нуль",
    );
    check("vehicle plate", normalize("АА 1234 КВ"), "номерний знак а а один два три чотири ка ве");
    check(
        "crypto amount",
        normalize("0,5 BTC і 2 ETH"),
        "нуль цілих і п'ять десятих біткоїна і два ефіри",
    );
    check(
        "exchange pair",
        normalize("BTC/UAH та USD/UAH"),
        "біткоїнів до гривень та доларів США до гривень",
    );
    check("social handle", normalize("@OpenAI"), "акаунт опеней");
    check(
        "brand exceptions",
        normalize("OpenAI, ChatGPT, GitHub і MacBook"),
        "опеней, чатджипіті, гітхаб і макбук",
    );
    check(
        "product exceptions",
        normalize("iPhone, YouTube, Docker і Kubernetes"),
        "айфон, ютуб, докер і кубернетіс",
    );
    let mut custom_words = NormalizeOptions::default();
    custom_words.vocabulary = [("google", "гуголь"), ("acme", "акме")]
        .into_iter()
        .map(|(k, v): (&str, &str)| (k.to_owned(), v.to_owned()))
        .collect();
    check(
        "custom vocabulary overrides built-in word",
        normalize_with("Google і Acme", &custom_words),
        "гуголь і акме",
    );
    check("custom vocabulary stays local to options", normalize("Google"), "гугл");
    check("url query", normalize("https://example.com/a?x=1&y=2"), "гттпс двокрапка слеш слеш ексампле крапка ком слеш а знак питання кс дорівнює один амперсанд и дорівнює два");
    check(
        "court case number",
        normalize("справа № 910/1234/24"),
        "справа номер дев'ятсот десять слеш тисяча двісті тридцять чотири слеш двадцять чотири",
    );
    check(
        "proceeding number",
        normalize("провадження № 61-12345св24"),
        "провадження номер шістдесят один дефіс один два три чотири п'ять ес ве двадцять чотири",
    );
    check("government resolution number", normalize("Постанова КМУ № 123/2026-р"), "Постанова кабінет міністрів України номер сто двадцять три слеш дві тисячі двадцять шість дефіс ер");
    check(
        "inflected case legal number",
        normalize("у справі № 910/1234/24"),
        "у справі номер дев'ятсот десять слеш тисяча двісті тридцять чотири слеш двадцять чотири",
    );
    check(
        "law roman suffix",
        normalize("Закон № 1402-VIII"),
        "Закон номер тисяча чотириста два дефіс восьмий",
    );
    check(
        "law genitive roman suffix",
        normalize("Закону № 1402-VIII"),
        "Закону номер тисяча чотириста два дефіс восьмий",
    );
    check("erdr number", normalize("ЄРДР. № 12024100000000000"), "єдиний реєстр досудових розслідувань номер один два нуль два чотири один нуль нуль нуль нуль нуль нуль нуль нуль нуль нуль нуль");
    check(
        "passport number",
        normalize("паспорт КВ 123456"),
        "паспорт ка ве один два три чотири п'ять шість",
    );
    check(
        "masked card",
        normalize("картка 4149 **** **** 1234"),
        "картка чотири один чотири дев'ять зірочки зірочки один два три чотири",
    );
    check(
        "masked card accusative",
        normalize("на картку 4149 **** **** 1234"),
        "на картку чотири один чотири дев'ять зірочки зірочки один два три чотири",
    );
    check("full card grouped", normalize("картка 4149 1234 5678 9012"), "картка чотири один чотири дев'ять один два три чотири п'ять шість сім вісім дев'ять нуль один два");
    check("order number reference", normalize("Замовлення №10"), "Замовлення номер десять");
    check("medical concentration", normalize("5 мг/мл"), "п'ять міліграмів на мілілітр");
    check(
        "dot decimal medical concentration",
        normalize("5.5 мг/мл"),
        "п'ять цілих і п'ять десятих міліграма на мілілітр",
    );
    check("medical frequency", normalize("2 рази на день"), "два рази на день");
    check(
        "medical temperature",
        normalize("37,5°C"),
        "тридцять сім цілих і п'ять десятих градуса Цельсія",
    );
    check(
        "dot decimal medical temperature",
        normalize("37.5°C"),
        "тридцять сім цілих і п'ять десятих градуса Цельсія",
    );
    check(
        "blood pressure",
        normalize("120/80 мм рт. ст."),
        "сто двадцять на вісімдесят міліметрів ртутного стовпа",
    );
    check(
        "labelled blood pressure",
        normalize("тиск 120/80 мм рт. ст."),
        "тиск сто двадцять на вісімдесят міліметрів ртутного стовпа",
    );
    check("package number", normalize("препарат №10"), "препарат номер десять");
    let mut conservative = NormalizeOptions::default();
    conservative.expand_known_acronyms = false;
    conservative.spell_unknown_acronyms = false;
    conservative.normalize_english_words = false;
    conservative.transliterate_latin = false;
    check(
        "conservative options",
        normalize_with("Python 3 і ФОП", &conservative),
        "Python три і ФОП",
    );
    check(
        "conservative brand options",
        normalize_with("OpenAI і ChatGPT", &conservative),
        "OpenAI і ChatGPT",
    );
    let mut range_options = NormalizeOptions::default();
    range_options.range_style = RangeStyle::FromTo;
    check(
        "from-to unit range",
        normalize_with("5-7 кг", &range_options),
        "від п'яти до семи кілограмів",
    );
    check(
        "from-to en dash unit range",
        normalize_with("5–7 кг", &range_options),
        "від п'яти до семи кілограмів",
    );
    check(
        "from-to percent range",
        normalize_with("10-15%", &range_options),
        "від десяти до п'ятнадцяти відсотків",
    );
    check(
        "prepositional year range",
        normalize_with("У 1998—2000 роках.", &range_options),
        "У період від тисяча дев'ятсот дев'яносто восьмого до двохтисячного року.",
    );
    check("prepositional bare year range", normalize_with("У 1950—1951, за рекомендацією, було обране місце.", &range_options), "У період від тисяча дев'ятсот п'ятдесятого до тисяча дев'ятсот п'ятдесят першого року, за рекомендацією, було обране місце.");
    check(
        "range after explicit vid",
        normalize_with("Енергія менша від 1,5–2 еВ.", &range_options),
        "Енергія менша від однієї цілої і п'яти десятих до двох електронвольтів.",
    );
    check(
        "approximate range after ponad",
        normalize_with("понад 300—400 рядків", &range_options),
        "понад триста чи чотириста рядків",
    );
    for input in [
        "5-7 °C",
        "5–7 °C",
        "5—7 °C",
        "5 - 7 °C",
        "5-7°C",
        "5–7°C",
        "5–7 °С",
        "5–7 °с",
        "5-7 градусів Цельсія",
        "5–7 градусів Цельсія",
        "5-7 градусів цельсія",
        "5-7 ГРАДУСІВ ЦЕЛЬСІЯ",
        "5-7 градусів C",
        "5–7 градусів за Цельсієм",
    ] {
        check(
            &format!("from-to temperature range {input}"),
            normalize_with(input, &range_options),
            "від п'яти до семи градусів Цельсія",
        );
    }
    check(
        "decimal temperature range",
        normalize_with("5,5–7,5 °C", &range_options),
        "від п'яти цілих і п'яти десятих до семи цілих і п'яти десятих градуса Цельсія",
    );
    check(
        "fahrenheit temperature range",
        normalize_with("5–7 °F", &range_options),
        "від п'яти до семи градусів Фаренгейта",
    );
    check(
        "unicode temperature symbols",
        normalize_with("5–7 ℃ і 8–9 ℉", &range_options),
        "від п'яти до семи градусів Цельсія і від восьми до дев'яти градусів Фаренгейта",
    );
    check(
        "standalone kelvin",
        normalize_with("273 K", &range_options),
        "двісті сімдесят три кельвіни",
    );
    check(
        "governed kelvin",
        normalize("Речовину нагріли до 300 K."),
        "Речовину нагріли до трьохсот кельвінів.",
    );
    check(
        "governed celsius",
        normalize("Температура зросла до 500 °С."),
        "Температура зросла до п'ятисот градусів Цельсія.",
    );
    check(
        "unicode kelvin sign",
        normalize_with("273 \u{212a}", &range_options),
        "двісті сімдесят три кельвіни",
    );
    check(
        "legacy degree kelvin",
        normalize_with("273 °K", &range_options),
        "двісті сімдесят три кельвіни",
    );
    check(
        "kelvin range",
        normalize_with("250–300 K", &range_options),
        "від двохсот п'ятдесяти до трьохсот кельвінів",
    );
    check(
        "signed kelvin range",
        normalize_with("-5–+7 K", &range_options),
        "від мінус п'яти до плюс семи кельвінів",
    );
    check(
        "decimal kelvin range",
        normalize_with("1,5–2,5 K", &range_options),
        "від однієї цілої і п'яти десятих до двох цілих і п'яти десятих кельвіна",
    );
    check(
        "repeated kelvin range",
        normalize_with("від 250 K до 300 K", &range_options),
        "від двохсот п'ятдесяти до трьохсот кельвінів",
    );
    check(
        "rankine range",
        normalize_with("5–7 °R", &range_options),
        "від п'яти до семи градусів Ранкіна",
    );
    check(
        "reaumur range",
        normalize_with("5–7 °Ré", &range_options),
        "від п'яти до семи градусів Реомюра",
    );
    check(
        "delisle range",
        normalize_with("5–7 °De", &range_options),
        "від п'яти до семи градусів Деліля",
    );
    check(
        "romer range",
        normalize_with("5–7 °Rø", &range_options),
        "від п'яти до семи градусів Ремера",
    );
    check(
        "newton named range",
        normalize_with("5–7 градусів Ньютона", &range_options),
        "від п'яти до семи градусів Ньютона",
    );
    check(
        "mixed temperature scales",
        normalize_with("5 °C–7 K", &range_options),
        "від п'яти градусів Цельсія до семи кельвінів",
    );
    check(
        "repeated temperature units",
        normalize_with("5°C–7°C", &range_options),
        "від п'яти до семи градусів Цельсія",
    );
    check(
        "explicit unsigned temperature range",
        normalize_with("від 5 до 7 °C", &range_options),
        "від п'яти до семи градусів Цельсія",
    );
    check(
        "explicit repeated named temperature range",
        normalize_with("від -5 градусів Цельсія до +7 градусів Цельсія", &range_options),
        "від мінус п'яти до плюс семи градусів Цельсія",
    );
    check(
        "negative temperature",
        normalize_with("-5 °C", &range_options),
        "мінус п'ять градусів Цельсія",
    );
    check(
        "unicode minus temperature",
        normalize_with("−5 °C", &range_options),
        "мінус п'ять градусів Цельсія",
    );
    check(
        "en dash unary minus temperature",
        normalize_with("–5 °C", &range_options),
        "мінус п'ять градусів Цельсія",
    );
    check(
        "signed decimal temperature",
        normalize_with("-5,5 °C", &range_options),
        "мінус п'ять цілих і п'ять десятих градуса Цельсія",
    );
    check(
        "explicit signed temperature range",
        normalize_with("від -5 до +7 °C", &range_options),
        "від мінус п'яти до плюс семи градусів Цельсія",
    );
    check(
        "descending temperature range",
        normalize_with("7–5 °C", &range_options),
        "від семи до п'яти градусів Цельсія",
    );
    check(
        "equal temperature range",
        normalize_with("5–5 °C", &range_options),
        "від п'яти до п'яти градусів Цельсія",
    );
    check_absent(
        "oversized temperature range",
        normalize_with("999999999999999999999999–1000000000000000000000000 °C", &range_options),
        "від нуля до нуля",
    );
    check(
        "signed unit range",
        normalize_with("-5–7 кг", &range_options),
        "від мінус п'яти до семи кілограмів",
    );
    check(
        "decimal unit range",
        normalize_with("1,5–2,5 кг", &range_options),
        "від однієї цілої і п'яти десятих до двох цілих і п'яти десятих кілограмів",
    );
    check(
        "repeated unit range",
        normalize_with("1 кг–2 кг", &range_options),
        "від одного до двох кілограмів",
    );
    check(
        "explicit repeated unit range",
        normalize_with("від 1 кг до 2 кг", &range_options),
        "від одного до двох кілограмів",
    );
    check(
        "repeated decimal percent range",
        normalize_with("10,5%–15,5%", &range_options),
        "від десяти цілих і п'яти десятих до п'ятнадцяти цілих і п'яти десятих відсотків",
    );
    check(
        "explicit repeated percent range",
        normalize_with("від 10% до 15%", &range_options),
        "від десяти до п'ятнадцяти відсотків",
    );
    check(
        "currency suffix range",
        normalize_with("5–7 грн", &range_options),
        "від п'яти до семи гривень",
    );
    check(
        "currency prefix range",
        normalize_with("$5–$7", &range_options),
        "від п'яти до семи доларів",
    );
    check(
        "explicit repeated currency range",
        normalize_with("від 5 грн до 7 грн", &range_options),
        "від п'яти до семи гривень",
    );
    check("bare number range", normalize_with("5–7", &range_options), "від п'яти до семи");
    check(
        "terminal bare number range",
        normalize_with("5–7.", &range_options),
        "від п'яти до семи.",
    );
    check("range after punctuation dash", normalize_with("У Європі — 300—330, 380—400 кВ.", &range_options), "У Європі — від трьохсот до трьохсот тридцяти, від трьохсот вісімдесяти до чотирьохсот кіловольт.");
    check(
        "time range",
        normalize_with("10:30–12:45", &range_options),
        "від десятої години тридцяти хвилин до дванадцятої години сорока п'яти хвилин",
    );
    check(
        "fraction range",
        normalize_with("1/2–3/4", &range_options),
        "від однієї другої до трьох четвертих",
    );
    check(
        "page range",
        normalize_with("стор. 5–7", &range_options),
        "від п'ятої до сьомої сторінки",
    );
    check(
        "short page range",
        normalize_with("с. 5–7", &range_options),
        "від п'ятої до сьомої сторінки",
    );
    check(
        "uppercase page range",
        normalize_with("Стор. 5—7", &range_options),
        "від п'ятої до сьомої сторінки",
    );
    check(
        "English bibliographic page range",
        normalize_with("P. 1227–1246.", &range_options),
        "від тисяча двісті двадцять сьомої до тисяча двісті сорок шостої сторінки.",
    );
    check(
        "legal article range",
        normalize_with("ст. 5–7", &range_options),
        "від п'ятої до сьомої статті",
    );
    check(
        "legal point range",
        normalize_with("п. 2-4", &range_options),
        "від другого до четвертого пункту",
    );
    check(
        "year month",
        normalize_with("2026-09", &range_options),
        "вересень дві тисячі двадцять шостого року",
    );
    check_absent(
        "invalid year month is not a range",
        normalize_with("2026-13", &range_options),
        "від",
    );
    check(
        "short DMY date",
        normalize_with("14.09.26", &range_options),
        "чотирнадцяте вересня дві тисячі двадцять шостого року",
    );
    check("ISO datetime", normalize_with("2026-09-14T10:30:00Z", &range_options), "чотирнадцяте вересня дві тисячі двадцять шостого року о десять годин тридцять хвилин за всесвітнім координованим часом");
    check(
        "ISO duration",
        normalize("P1Y2M3DT4H5M6S"),
        "один рік два місяці три дні чотири години п'ять хвилин шість секунд",
    );
    check(
        "ISO week date",
        normalize("2026-W37-1"),
        "перший день тридцять сьомого тижня дві тисячі двадцять шостого року",
    );
    check(
        "ISO ordinal date",
        normalize("2024-366"),
        "триста шістдесят шостий день дві тисячі двадцять четвертого року",
    );
    check(
        "IANA timezone",
        normalize("10:30 Europe/Kyiv"),
        "десять годин тридцять хвилин за київським часом",
    );
    check("PM time", normalize("10:30 PM"), "десять годин тридцять хвилин вечора");
    check("midnight", normalize("00:00"), "опівночі");
    check("ratio", normalize("16:9"), "шістнадцять до дев'яти");
    check(
        "scientific e notation",
        normalize("1e-3"),
        "один помножити на десять у степені мінус три",
    );
    check(
        "scientific superscript",
        normalize("6.02×10²³"),
        "шість цілих і дві сотих помножити на десять у степені двадцять три",
    );
    check("negative fraction", normalize("-1/2"), "мінус одна друга");
    check("zero denominator preserved", normalize("1/0"), "один/нуль");
    check("signed prefix currency", normalize("-$5"), "мінус п'ять доларів");
    check("accounting currency", normalize("(100 грн)"), "мінус сто гривень");
    check("currency code prefix", normalize("USD 10"), "десять доларів");
    check("repeated currency amounts", normalize("5 грн і 6 грн"), "п'ять гривень і шість гривень");
    check("additional fiat", normalize("2 KRW"), "дві вони");
    check("additional crypto", normalize("0.5 DOGE"), "нуль цілих і п'ять десятих доджкоїна");
    check(
        "fuel economy",
        normalize("6.5 L/100km"),
        "шість цілих і п'ять десятих літра на сто кілометрів",
    );
    check("imperial unit", normalize("12 oz"), "дванадцять унцій");
    check("torque unit", normalize("10 Н·м"), "десять ньютон-метрів");
    check("composed force unit", normalize("20 кг·м/с²"), "двадцять ньютонів");
    check("viscosity unit", normalize("0,5 Па·с"), "нуль цілих і п'ять десятих паскаль-секунди");
    check(
        "EV energy unit",
        normalize("18 кВт·год/100 км"),
        "вісімнадцять кіловат-годин на сто кілометрів",
    );
    check(
        "composable unit fallback",
        normalize("7 кг·м/с³"),
        "сім кілограмів помножити на метр поділити на секунду у кубі",
    );
    check(
        "compact measurement tolerance",
        normalize("5±0,2 кг"),
        "п'ять плюс мінус нуль цілих і дві десятих кілограма",
    );
    check(
        "percentage tolerance",
        normalize("5 кг ± 2%"),
        "п'ять кілограмів плюс мінус два відсотки",
    );
    check(
        "IPv4 endpoint",
        normalize("192.168.1.1:8080"),
        "ай пі сто дев'яносто два сто шістдесят вісім один один порт вісім тисяч вісімдесят",
    );
    check(
        "IPv4 CIDR",
        normalize("10.0.0.0/24"),
        "ай пі десять нуль нуль нуль префікс двадцять чотири",
    );
    check("MAC address", normalize("AA:BB:CC:DD:EE:FF"), "мак адреса ей ей двокрапка бі бі двокрапка сі сі двокрапка ді ді двокрапка і і двокрапка еф еф");
    check("decimal coordinates", normalize("50.4501 N, 30.5234 E"), "п'ятдесят цілих і чотири тисячі п'ятсот одна десятитисячна градуса північної широти, тридцять цілих і п'ять тисяч двісті тридцять чотири десятитисячних градуса східної довготи");
    check_absent("invalid decimal coordinates", normalize("90.1 N"), "північної широти");
    check_absent("invalid DMS coordinates", normalize("50°99′00″N"), "північної широти");
    check("geo URI", normalize("geo:-33.8688,151.2093,58"), "географічні координати: тридцять три цілих і вісім тисяч шістсот вісімдесят вісім десятитисячних градуса південної широти, сто п'ятдесят одна ціла і дві тисячі дев'яносто три десятитисячних градуса східної довготи, висота п'ятдесят вісім метрів");
    check(
        "decimal minute coordinate",
        normalize("50°27,5′N"),
        "п'ятдесят градусів двадцять сім цілих і п'ять десятих хвилини північної широти",
    );
    check("UUID", normalize("550e8400-e29b-41d4-a716-446655440000"), "ю у ай ді п'ять п'ять нуль і вісім чотири нуль нуль дефіс і два дев'ять бі дефіс чотири один ді чотири дефіс ей сім один шість дефіс чотири чотири шість шість п'ять п'ять чотири чотири нуль нуль нуль нуль");
    check(
        "ISBN",
        normalize("ISBN 978-617-123-456-7"),
        "ай ес бі ен дев'ять сім вісім шість один сім один два три чотири п'ять шість сім",
    );
    check(
        "ISSN",
        normalize("ISSN 1234-567X"),
        "ай ес ес ен один два три чотири п'ять шість сім екс",
    );
    check("VIN", normalize("VIN WVWZZZ1JZXW000001"), "він номер дабл ю ві дабл ю зед зед зед один джей зед екс дабл ю нуль нуль нуль нуль нуль один");
    check(
        "SWIFT",
        normalize("SWIFT DEUTDEFF500"),
        "свіфт код ді і ю ті ді і еф еф п'ять нуль нуль",
    );
    check("foreign IBAN", normalize("DE89 3704 0044 0532 0130 00"), "айбан ді і вісім дев'ять три сім нуль чотири нуль нуль чотири чотири нуль п'ять три два нуль один три нуль нуль нуль");
    check("international access phone", normalize_with("0044 20 7946 0958 ext 5", &range_options), "плюс сорок чотири двадцять сім дев'ять чотири шість нуль дев'ять п'ять вісім додатковий п'ять");
    check(
        "FTP arbitrary TLD",
        normalize("ftp://example.dev/a#b"),
        "фтп двокрапка слеш слеш ексампле крапка дев слеш а решітка б",
    );
    check(
        "Ukrainian domain label",
        normalize("Сайт ts.kiev.ua працює."),
        "Сайт ц крапка кіев крапка ю ей працює.",
    );
    check(
        "standalone Ukrainian ASCII domain",
        normalize("Домен .UA делеговано."),
        "Домен крапка ю ей делеговано.",
    );
    check(
        "standalone Ukrainian IDN domain",
        normalize("Домен .укр делеговано."),
        "Домен крапка укр делеговано.",
    );
    check("SSML preserved", normalize("<speak>5 кг</speak>"), "<speak>п'ять кілограмів</speak>");
    check(
        "inline code preserved",
        normalize("Код `x=5`, вага 2 кг"),
        "Код `x=5`, вага два кілограми",
    );
    check(
        "MediaWiki display math preserved",
        normalize(r"Формула {\displaystyle E=mc^{2}}, вага 5 кг."),
        r"Формула {\displaystyle E=mc^{2}}, вага п'ять кілограмів.",
    );
    check("IPA preserved", normalize("OS МФА: [oʊˈɛs]"), "оу ес МФА: [oʊˈɛs]");
    check(
        "Latin diacritics transliterated",
        normalize("Plankalkül, Vigenère, computār"),
        "планкалкюл, вігенере, компутар",
    );
    check("isolated Latin diacritic transliterated", normalize("Квáнтовий"), "Квантовий");
    check(
        "Markdown destination and entity preserved",
        normalize("[5 кг](https://example.com/a?x=1&amp;y=2)"),
        "[п'ять кілограмів](https://example.com/a?x=1&amp;y=2)",
    );
    let compact_range_options = NormalizeOptions::default();
    check(
        "compact temperature range",
        normalize_with("-5–-3 °F", &compact_range_options),
        "мінус п'ять мінус три градусів Фаренгейта",
    );
    let mut phone_options = NormalizeOptions::default();
    phone_options.phone_style = PhoneStyle::DigitByDigit;
    check(
        "phone digit by digit",
        normalize_with("+380 67 123-45-67", &phone_options),
        "плюс три вісім нуль шість сім один два три чотири п'ять шість сім",
    );
    let mut symbol_options = NormalizeOptions::default();
    symbol_options.symbol_style = SymbolStyle::Preserve;
    check(
        "preserve symbols",
        normalize_with("2 + 2 = 4 і 50%", &symbol_options),
        "два + два = чотири і п'ятдесят відсотків",
    );
    check("expand symbols default", normalize("2 + 2 = 4"), "два плюс два дорівнює чотири");
    let mut spoken_dates = NormalizeOptions::default();
    spoken_dates.date_style = DateStyle::Spoken;
    check(
        "spoken numeric date",
        normalize_with("15.06.2026", &spoken_dates),
        "п'ятнадцятого червня дві тисячі двадцять шостого року",
    );
    check(
        "spoken iso date",
        normalize_with("2026-06-15", &spoken_dates),
        "п'ятнадцятого червня дві тисячі двадцять шостого року",
    );
    check("spoken numeric date range", normalize_with("15.06.2026-16.06.2026", &spoken_dates), "п'ятнадцятого червня дві тисячі двадцять шостого року шістнадцятого червня дві тисячі двадцять шостого року");
    spoken_dates.range_style = RangeStyle::FromTo;
    check(
        "spoken en dash day range",
        normalize_with("15–16 вересня 2026", &spoken_dates),
        "від п'ятнадцятого до шістнадцятого вересня дві тисячі двадцять шостого року",
    );
    check("spoken full date range from-to", normalize_with("15.06.2026–16.06.2026", &spoken_dates), "від п'ятнадцятого червня дві тисячі двадцять шостого року до шістнадцятого червня дві тисячі двадцять шостого року");
    check(
        "year range from-to",
        normalize_with("2020–2024 рр.", &spoken_dates),
        "від дві тисячі двадцятого до дві тисячі двадцять четвертого року.",
    );
    let mut tts_options = NormalizeOptions::default();
    tts_options.range_style = RangeStyle::FromTo;
    tts_options.phone_style = PhoneStyle::DigitByDigit;
    tts_options.date_style = DateStyle::Spoken;
    check(
        "tts preset",
        normalize_preset("15.06.2026, +380 67 123-45-67, 5-7 кг", NormalizePreset::TtsFriendly),
        normalize_with("15.06.2026, +380 67 123-45-67, 5-7 кг", &tts_options),
    );
    check(
        "explicit preset API",
        normalize_preset("OpenAI + ФОП", NormalizePreset::SearchIndexing),
        "OpenAI + фізична особа підприємець",
    );
    {
        let mut ambiguity = NormalizeOptions::default();
        ambiguity.colon_style = ColonStyle::Ratio;
        check("forced colon ratio", normalize_with("10:30", &ambiguity), "десять до тридцяти");
        ambiguity.colon_style = ColonStyle::Clock;
        check(
            "forced colon clock",
            normalize_with("10:30", &ambiguity),
            "десять годин тридцять хвилин",
        );
        ambiguity.numeric_date_order = NumericDateOrder::MonthDayYear;
        check(
            "month day year policy",
            normalize_with("03/04/2026", &ambiguity),
            "четверте березня дві тисячі двадцять шостого року",
        );
        ambiguity.numeric_date_order = NumericDateOrder::PreserveAmbiguous;
        check(
            "preserve ambiguous numeric date",
            normalize_with("03/04/2026", &ambiguity),
            "03/04/2026",
        );
        ambiguity.currency_symbol_policy = CurrencySymbolPolicy::PreserveAmbiguous;
        check(
            "preserve ambiguous currency symbols",
            normalize_with("$12 і ¥500", &ambiguity),
            "$дванадцять і ¥п'ятсот",
        );
    }
    check(
        "preset helper conservative",
        normalize_preset("OpenAI + ФОП", NormalizePreset::Conservative),
        "OpenAI + ФОП",
    );
    check(
        "preset helper search",
        normalize_preset("OpenAI + ФОП", NormalizePreset::SearchIndexing),
        "OpenAI + фізична особа підприємець",
    );
    check("oversized standalone number", normalize("Номер 123456789012345678901234567890"), "Номер один два три чотири п'ять шість сім вісім дев'ять нуль один два три чотири п'ять шість сім вісім дев'ять нуль один два три чотири п'ять шість сім вісім дев'ять нуль");
    check("oversized dotted version", normalize("Версія 999999999999999999999999.1.2"), "Версія дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять крапка один крапка два");
    check(
        "version dotted quad",
        normalize("Версія 1.2.3.4"),
        "Версія один крапка два крапка три крапка чотири",
    );
    check("oversized percent", normalize("Знижка 999999999999999999999999%"), "Знижка дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять відсотків");
    check("oversized measurement", normalize("Вага 999999999999999999999999 кг"), "Вага дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять дев'ять кілограмів");
    check(
        "overprecise currency decimal",
        normalize("Сума 1,23456789 грн"),
        "Сума один кома два три чотири п'ять шість сім вісім дев'ять грн",
    );
    let uncertain = flag_uncertain("У 2024 вийшов Foo X.");
    check_span_at("uncertain year", &uncertain, 0, "2024");
    check_span_at("uncertain latin", &uncertain, 1, "Foo");
    check_span_meta(
        "uncertain latin metadata",
        &uncertain,
        "Foo",
        UncertaintyCategory::ForeignWord,
        UncertaintySeverity::Info,
    );
    let more_uncertain = flag_uncertain("Подія 32.13.2024. Див. ст. 5 та FooКиїв IX.");
    check_span_reason("uncertain invalid date", &more_uncertain, "32.13.2024", "numeric date");
    check_span_meta(
        "uncertain invalid date metadata",
        &more_uncertain,
        "32.13.2024",
        UncertaintyCategory::Date,
        UncertaintySeverity::Error,
    );
    check_span_reason(
        "uncertain ambiguous abbreviation",
        &more_uncertain,
        "ст.",
        "ambiguous abbreviation",
    );
    check_span_reason(
        "uncertain bare number",
        &flag_uncertain("Є 7 варіантів."),
        "7",
        "bare number",
    );
    check_span_reason("uncertain mixed word", &more_uncertain, "FooКиїв", "mixed-script");
    check_span_meta(
        "uncertain mixed metadata",
        &more_uncertain,
        "FooКиїв",
        UncertaintyCategory::MixedScript,
        UncertaintySeverity::Error,
    );
    check_span_reason("uncertain roman", &more_uncertain, "IX", "Roman numeral");
    check_span_meta(
        "uncertain identifier metadata",
        &flag_uncertain("справа № 910/1234/24"),
        "№ 910/1234/24",
        UncertaintyCategory::Identifier,
        UncertaintySeverity::Info,
    );
    check_span_meta(
        "uncertain full card metadata",
        &flag_uncertain("картка 4149 1234 5678 9012"),
        "картка 4149 1234 5678 9012",
        UncertaintyCategory::Identifier,
        UncertaintySeverity::Error,
    );
    check_no_span_meta(
        "valid card checksum",
        &flag_uncertain("картка 4111 1111 1111 1111"),
        "картка 4111 1111 1111 1111",
        UncertaintySeverity::Error,
    );
    check_no_span_meta(
        "valid ISBN-13 checksum",
        &flag_uncertain("ISBN 978-0-306-40615-7"),
        "ISBN 978-0-306-40615-7",
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid ISBN-13 checksum",
        &flag_uncertain("ISBN 978-0-306-40615-8"),
        "ISBN 978-0-306-40615-8",
        UncertaintyCategory::Identifier,
        UncertaintySeverity::Error,
    );
    check_no_span_meta(
        "valid ISBN-10 checksum",
        &flag_uncertain("ISBN-10 0-306-40615-2"),
        "ISBN-10 0-306-40615-2",
        UncertaintySeverity::Error,
    );
    check_no_span_meta(
        "valid ISSN checksum",
        &flag_uncertain("ISSN 0317-8471"),
        "ISSN 0317-8471",
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid ISSN checksum",
        &flag_uncertain("ISSN 0317-8472"),
        "ISSN 0317-8472",
        UncertaintyCategory::Identifier,
        UncertaintySeverity::Error,
    );
    check_no_span_meta(
        "valid IBAN checksum",
        &flag_uncertain("DE89 3704 0044 0532 0130 00"),
        "DE89 3704 0044 0532 0130 00",
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid IBAN checksum",
        &flag_uncertain("DE88 3704 0044 0532 0130 00"),
        "DE88 3704 0044 0532 0130 00",
        UncertaintyCategory::Identifier,
        UncertaintySeverity::Error,
    );
    check_no_span_meta(
        "valid VIN checksum",
        &flag_uncertain("VIN 1M8GDM9AXKP042788"),
        "VIN 1M8GDM9AXKP042788",
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid VIN checksum",
        &flag_uncertain("VIN 1M8GDM9A1KP042788"),
        "VIN 1M8GDM9A1KP042788",
        UncertaintyCategory::Identifier,
        UncertaintySeverity::Error,
    );
    check_no_span_meta(
        "valid UUID version and variant",
        &flag_uncertain("550e8400-e29b-41d4-a716-446655440000"),
        "550e8400-e29b-41d4-a716-446655440000",
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid UUID variant",
        &flag_uncertain("550e8400-e29b-41d4-0716-446655440000"),
        "550e8400-e29b-41d4-0716-446655440000",
        UncertaintyCategory::Identifier,
        UncertaintySeverity::Error,
    );
    check_no_span_meta(
        "valid hash length",
        &flag_uncertain("MD5 d41d8cd98f00b204e9800998ecf8427e"),
        "MD5 d41d8cd98f00b204e9800998ecf8427e",
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid hash length",
        &flag_uncertain("MD5 d41d8cd98f00b204"),
        "MD5 d41d8cd98f00b204",
        UncertaintyCategory::Identifier,
        UncertaintySeverity::Error,
    );
    check_no_category(
        "supported ISO currency metadata",
        &flag_uncertain("Сума 12 AED."),
        UncertaintyCategory::Currency,
    );
    check_no_category(
        "generic finance ticker is not an unknown unit",
        &flag_uncertain("Сума 5 XYZ."),
        UncertaintyCategory::Unit,
    );
    check_span_meta(
        "ambiguous numeric date metadata",
        &flag_uncertain("Дата 03/04/2026"),
        "03/04/2026",
        UncertaintyCategory::Date,
        UncertaintySeverity::Warning,
    );
    check_span_meta(
        "ambiguous colon metadata",
        &flag_uncertain("Значення 10:30"),
        "10:30",
        UncertaintyCategory::Time,
        UncertaintySeverity::Warning,
    );
    check_span_meta(
        "ambiguous currency symbol metadata",
        &flag_uncertain("Сума $12"),
        "$12",
        UncertaintyCategory::Currency,
        UncertaintySeverity::Warning,
    );
    check_span_meta(
        "uncertain unit metadata",
        &flag_uncertain("Вага 5 qq."),
        "5 qq",
        UncertaintyCategory::Unit,
        UncertaintySeverity::Warning,
    );
    check_no_category(
        "UTF-8 tonne abbreviation is a known unit",
        &flag_uncertain("Енциклопедія у 3 т."),
        UncertaintyCategory::Unit,
    );
    check_no_category(
        "a year followed by a preposition is not a unit",
        &flag_uncertain("2016 у Wayback Machine"),
        UncertaintyCategory::Unit,
    );
    check_no_category(
        "a compound data rate is a known unit",
        &flag_uncertain("100 Мбіт/с"),
        UncertaintyCategory::Unit,
    );
    check_no_category(
        "a mixed-script data rate is a known unit",
        &flag_uncertain("10 Гбіт/c"),
        UncertaintyCategory::Unit,
    );
    check_no_category(
        "a month after a date is ordinary prose",
        &flag_uncertain("1 січня"),
        UncertaintyCategory::Unit,
    );
    check_no_category(
        "a chemical formula is not a number followed by a unit",
        &flag_uncertain("H2O"),
        UncertaintyCategory::Unit,
    );
    check_no_category(
        "3G is a network generation, not an unknown unit",
        &flag_uncertain("мережа 3G"),
        UncertaintyCategory::Unit,
    );
    check_span_meta(
        "uncertain email metadata",
        &flag_uncertain("Контакт test@"),
        "test@",
        UncertaintyCategory::Web,
        UncertaintySeverity::Warning,
    );
    check_span_meta(
        "uncertain url metadata",
        &flag_uncertain("Перейти на https://"),
        "https://",
        UncertaintyCategory::Web,
        UncertaintySeverity::Warning,
    );
    {
        let conservative = NormalizeOptions::preset(NormalizePreset::Conservative);
        check(
            "homoglyphs off in conservative",
            normalize_with("Пoлтaвa", &conservative),
            "Пoлтaвa",
        );
        let mut no_validation = NormalizeOptions::default();
        no_validation.validate_dates = false;
        check("invalid date rejected", normalize("тридцять 30.02.2024"), "тридцять 30.02.2024");
        check(
            "invalid date accepted when validation off",
            normalize_with("30.02.2024", &no_validation),
            "тридцяте лютого дві тисячі двадцять четвертого року",
        );
        let mut strip_quotes = NormalizeOptions::default();
        strip_quotes.quote_style = QuoteStyle::Strip;
        check("quote strip", normalize_with("Слово «тест» тут", &strip_quotes), "Слово тест тут");
        let mut straight_quotes = NormalizeOptions::default();
        straight_quotes.quote_style = QuoteStyle::Straight;
        check(
            "quote straight",
            normalize_with("Слово «тест» тут", &straight_quotes),
            "Слово \"тест\" тут",
        );
        let mut no_network = conservative.clone();
        no_network.normalize_network_addresses = false;
        check(
            "ip network opt-out",
            normalize_with("IP 192.168.100.200", &no_network),
            "IP сто дев'яносто два крапка сто шістдесят вісім крапка сто крапка двісті",
        );
    }
    check_span_meta(
        "uncertain invalid date metadata",
        &flag_uncertain("Дата 30.02.2024"),
        "30.02.2024",
        UncertaintyCategory::InvalidDate,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "uncertain number grouping metadata",
        &flag_uncertain("Сума 1,234"),
        "1,234",
        UncertaintyCategory::AmbiguousNumberGrouping,
        UncertaintySeverity::Warning,
    );
    check_span_meta(
        "uncertain unknown oblique agreement metadata",
        &flag_uncertain("у 4 фларбах"),
        "4 фларбах",
        UncertaintyCategory::Agreement,
        UncertaintySeverity::Info,
    );
    check_span_meta(
        "invalid time metadata",
        &flag_uncertain("Час 99:30"),
        "99:30",
        UncertaintyCategory::Time,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid AM PM metadata",
        &flag_uncertain("Час 13:30 PM"),
        "13:30 PM",
        UncertaintyCategory::Time,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid ISO date metadata",
        &flag_uncertain("Дата 2026-13-01"),
        "2026-13-01",
        UncertaintyCategory::InvalidDate,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid ISO week metadata",
        &flag_uncertain("Дата 2026-W54-8"),
        "2026-W54-8",
        UncertaintyCategory::InvalidDate,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid ISO ordinal metadata",
        &flag_uncertain("Дата 2025-366"),
        "2025-366",
        UncertaintyCategory::InvalidDate,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid timezone offset metadata",
        &flag_uncertain("Час UTC+24:00"),
        "UTC+24:00",
        UncertaintyCategory::Time,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid timezone minute metadata",
        &flag_uncertain("Час UTC+02:99"),
        "UTC+02:99",
        UncertaintyCategory::Time,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "unknown contextual IANA timezone metadata",
        &flag_uncertain("Час 10:30 Europe/Paris"),
        "Europe/Paris",
        UncertaintyCategory::Time,
        UncertaintySeverity::Warning,
    );
    check_no_category(
        "IANA timezone is not an unknown unit",
        &flag_uncertain("Час 10:30 Europe/Paris"),
        UncertaintyCategory::Unit,
    );
    check_no_category(
        "URL is not an IANA timezone",
        &flag_uncertain("https://example.com/a"),
        UncertaintyCategory::Time,
    );
    check_span_meta(
        "invalid geo URI metadata",
        &flag_uncertain("geo:91.2,181.0"),
        "geo:91.2,181.0",
        UncertaintyCategory::Coordinate,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid DMS coordinate metadata",
        &flag_uncertain("50°99′00″N"),
        "50°99′00″N",
        UncertaintyCategory::Coordinate,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "zero denominator metadata",
        &flag_uncertain("Частка 1/0"),
        "1/0",
        UncertaintyCategory::Fraction,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "invalid network metadata",
        &flag_uncertain("IP 999.1.1.1/40"),
        "999.1.1.1/40",
        UncertaintyCategory::Network,
        UncertaintySeverity::Error,
    );
    check_span_meta(
        "malformed scientific metadata",
        &flag_uncertain("Значення 1e+"),
        "1e+",
        UncertaintyCategory::Scientific,
        UncertaintySeverity::Warning,
    );
    let audit_options = NormalizeOptions::preset(NormalizePreset::TtsFriendly);
    check(
        "technical power with Unicode minus",
        normalize_with("10−9 м", &audit_options),
        "десять у степені мінус дев'ять метрів",
    );
    check(
        "technical power with a multiplier",
        normalize_with("2x10−6 м", &audit_options),
        "два помножити на десять у степені мінус шість метри",
    );
    check(
        "range of technical powers",
        normalize_with("10−15—10−12 секунди", &audit_options),
        "від десяти у степені мінус п'ятнадцять до десяти у степені мінус дванадцять секунд",
    );
    let greek_power = normalize_with("−0,0419·10−3ρh", &audit_options);
    check("power next to a Greek variable", &greek_power, "мінус нуль цілих і чотириста дев'ятнадцять десятитисячних помножити на десять у степені мінус три ро аш");
    check(
        "Greek-variable power is idempotent",
        normalize_with(&greek_power, &audit_options),
        &greek_power,
    );
    check(
        "inverse Celsius scientific unit",
        normalize_with("0,6 × 10−6°C−1", &audit_options),
        "нуль цілих і шість десятих помножити на десять у степені мінус шість на градус Цельсія",
    );
    check("Greek coefficient with an inverse unit", normalize_with("з α = 0,6 × 10−6°C−1", &audit_options), "з альфою, що дорівнює нуль цілих і шість десятих помножити на десять у степені мінус шість на градус Цельсія");
    check(
        "ordinary hyphen still denotes a range",
        normalize_with("10-12 м", &audit_options),
        "від десяти до дванадцяти метрів",
    );
    check(
        "Cyrillic Roman century",
        normalize_with("У ХХ ст.", &audit_options),
        "У двадцятому столітті",
    );
    check(
        "Cyrillic Roman century after a genitive cue",
        normalize_with("до початку ХХІ століття", &audit_options),
        "до початку двадцять першого століття",
    );
    check(
        "Cyrillic Roman century range",
        normalize_with("В Х—ХІ ст.", &audit_options),
        "В десятому–одинадцятому століттях",
    );
    check(
        "inflected Cyrillic Roman century",
        normalize_with("У ХХІ столітті", &audit_options),
        "У двадцять першому столітті",
    );
    check(
        "capitalized month in a full date",
        normalize_with("12 Січня 2013", &audit_options),
        "дванадцятого січня дві тисячі тринадцятого року",
    );
    check(
        "unambiguous US slash date",
        normalize_with("04/29/02", &audit_options),
        "двадцять дев'ятого квітня дві тисячі другого року",
    );
    check(
        "ambiguous slash date keeps local order",
        normalize_with("04/05/02", &audit_options),
        "четвертого травня дві тисячі другого року",
    );
    check(
        "genitive ordinal class",
        normalize_with("мережа 1 класу", &audit_options),
        "мережа першого класу",
    );
    check(
        "regional dollar with multiplier",
        normalize_with("US$2,9 трлн", &audit_options),
        "дві цілих і дев'ять десятих трильйона доларів",
    );
    check(
        "range after to",
        normalize_with("до 7-8 доларів", &audit_options),
        "до семи–восьми доларів",
    );
    check(
        "range after from",
        normalize_with("від 100…200 °С", &audit_options),
        "від ста–двохсот градусів Цельсія",
    );
    check(
        "range after on",
        normalize_with("на 60-80%", &audit_options),
        "на шістдесят–вісімдесят відсотків",
    );
    check(
        "range after near",
        normalize_with("близько 10%-15%", &audit_options),
        "близько десяти–п'ятнадцяти відсотків",
    );
    check("range after in", normalize_with("в 2—3 лінії", &audit_options), "в дві–три лінії");
    check(
        "range of school grades in locative",
        normalize_with("в 9-10 класах", &audit_options),
        "в дев'ятих–десятих класах",
    );
    check(
        "school grades after pupils are ordinal",
        normalize_with("Довідник для учнів 9-11 класів", &audit_options),
        "Довідник для учнів дев'ятих–одинадцятих класів",
    );
    check(
        "a count of classes remains cardinal",
        normalize_with("Школа має 9-11 класів", &audit_options),
        "Школа має від дев'яти до одинадцяти класів",
    );
    check(
        "abbreviated year range",
        normalize_with("У 1946–47 роках", &audit_options),
        "У період від тисяча дев'ятсот сорок шостого до тисяча дев'ятсот сорок сьомого року",
    );
    check(
        "abbreviated decade range",
        normalize_with("1970-80-х роках", &audit_options),
        "сімдесятих–вісімдесятих роках двадцятого століття",
    );
    check(
        "year range after on",
        normalize_with("на 2007—2010 роки", &audit_options),
        "на період від дві тисячі сьомого до дві тисячі десятого року",
    );
    check("date range after on", normalize_with("планували на 21-24 вересня 2020 р.", &audit_options), "планували на період від двадцять першого до двадцять четвертого вересня дві тисячі двадцятого року");
    check(
        "date range without a year",
        normalize_with("планували на 21-24 вересня", &audit_options),
        "планували на період від двадцять першого до двадцять четвертого вересня",
    );
    check("Bible chapter and verse do not become clock times", normalize_with("(Ісая 40:22, 40:28, 41:9)", &audit_options), "(Ісая розділ сорок, вірш двадцять два, розділ сорок, вірш двадцять вісім, розділ сорок один, вірш дев'ять)");
    check("unicode minus fraction", normalize_with("−1/2", &audit_options), "мінус одна друга");
    check(
        "signed compound measurement",
        normalize_with("-2,5 м/с²", &audit_options),
        "мінус дві цілих і п'ять десятих метра за секунду в квадраті",
    );
    check("signed percent", normalize_with("-5%", &audit_options), "мінус п'ять відсотків");
    check(
        "latin SI product",
        normalize_with("3 N*m", &audit_options),
        "три ньютони помножити на метр",
    );
    check(
        "latin radiative flux unit",
        normalize_with("3 W/m²", &audit_options),
        "три вати на квадратний метр",
    );
    check("ISO week duration", normalize_with("P2W", &audit_options), "два тижні");
    check(
        "fractional ISO duration",
        normalize_with("PT1.5H", &audit_options),
        "одна ціла і п'ять десятих години",
    );
    check(
        "fractional ISO day",
        normalize_with("P0.5D", &audit_options),
        "нуль цілих і п'ять десятих дня",
    );
    check(
        "ISO duration feminine agreement",
        normalize_with("PT1H30.5M", &audit_options),
        "одна година тридцять цілих і п'ять десятих хвилини",
    );
    check(
        "malformed leading-dot ISO duration preserved",
        normalize_with("PT.5H", &audit_options),
        "PT.5H",
    );
    check("invalid ISO duration preserved", normalize_with("P1DT", &audit_options), "P1DT");
    check("malformed scientific preserved", normalize_with("1e+", &audit_options), "1e+");
    check(
        "invalid ISBN preserved",
        normalize_with("ISBN 978-617-57-40-11-4", &audit_options),
        "ISBN 978-617-57-40-11-4",
    );
    check(
        "HTML code contents preserved",
        normalize_with("Формула <code>x = 5</code>, маса 2 кг.", &audit_options),
        "Формула <code>x = 5</code>, маса два кілограми.",
    );
    check(
        "mathematical comparisons are not HTML",
        normalize_with("l/h = 2…10 між (l/h < 2) і (l/h > 10).", &audit_options),
        "л/г дорівнює від двох до десяти між (л/г менше два) і (л/г більше десять).",
    );
    check(
        "stripped adjacent quotes keep word boundaries",
        normalize_with("дисертацію«Методична система»на тему", &audit_options),
        "дисертацію Методична система на тему",
    );
    check(
        "address abbreviation does not match inside word",
        normalize_with("пресс. сторінка двісті.", &audit_options),
        "пресс. сторінка двісті.",
    );
    check(
        "measurement abbreviation is not a city",
        normalize_with(&normalize_with("1 т. о. м. = 1 кг", &audit_options), &audit_options),
        "одна тонна. о. м. дорівнює один кілограм",
    );
    check(
        "acute apostrophe is canonicalized in an identifier",
        normalize_with("ICREPQ´04", &audit_options),
        "ай сі ар і пі к'ю'нуль чотири",
    );
    check(
        "high precision decimal is not a phone number",
        normalize_with("0,000000001 км", &audit_options),
        "нуль кома нуль нуль нуль нуль нуль нуль нуль нуль один кілометра",
    );
    check(
        "measurement after duration governor",
        normalize_with("протягом 4 хвилин", &audit_options),
        "протягом чотирьох хвилин",
    );
    check(
        "measurement after vprodovzh governor",
        normalize_with("Впродовж 15 хвилин очікуємо відбій тривоги.", &audit_options),
        "Впродовж п'ятнадцяти хвилин очікуємо відбій тривоги.",
    );
    check(
        "measurement after uprodovzh governor",
        normalize_with("упродовж 3 днів", &audit_options),
        "упродовж трьох днів",
    );
    check(
        "clock time after o takes locative",
        normalize_with("Зустріч о 10:30", &audit_options),
        "Зустріч о десятій годині тридцять хвилин",
    );
    check(
        "bare dot decimal",
        normalize_with("Коефіцієнт 0.9996.", &audit_options),
        "Коефіцієнт нуль цілих і дев'ять тисяч дев'ятсот дев'яносто шість десятитисячних.",
    );
    check(
        "named compact version",
        normalize_with("версії 2.6 і v0.9", &audit_options),
        "версії два крапка шість і ві нуль крапка дев'ять",
    );
    check(
        "single-letter standard",
        normalize_with("Стандарт E.214.", &audit_options),
        "Стандарт і крапка двісті чотирнадцять.",
    );
    check(
        "lettered construction standard",
        normalize_with("ДБН В.2.5-23:2010", &audit_options),
        "де бе ен ве крапка два крапка п'ять дефіс двадцять три двокрапка дві тисячі десять",
    );
    check(
        "numeric construction standard",
        normalize_with("ГОСТ 16483.17–81", &audit_options),
        "ГОСТ шістнадцять тисяч чотириста вісімдесят три крапка сімнадцять дефіс вісімдесят один",
    );
    let corpus_standards = normalize_with("IEEE 802 .22; ISO 8512-1:1990; ДСТУ ISO 80000-1:2016; ISO / IEC 7812; ISO-8859-1; ДНАОП 0.00-1.32-01.", &audit_options);
    check("Wikipedia technical standards", &corpus_standards, "ай і і і вісімсот два крапка двадцять два; ай ес оу вісім тисяч п'ятсот дванадцять дефіс один двокрапка тисяча дев'ятсот дев'яносто; ДСТУ ай ес оу вісімдесят тисяч дефіс один двокрапка дві тисячі шістнадцять; ай ес оу слеш ай і сі сім тисяч вісімсот дванадцять; ай ес оу дефіс вісім тисяч вісімсот п'ятдесят дев'ять дефіс один; ДНАОП нуль крапка нуль нуль дефіс один крапка тридцять два дефіс нуль один.");
    check(
        "technical standards are idempotent",
        normalize_with(&corpus_standards, &audit_options),
        &corpus_standards,
    );
    check("standard delimiter does not consume prose", normalize_with("IEC 61970/61968 — загальна модель.", &audit_options), "ай і сі шістдесят одна тисяча дев'ятсот сімдесят слеш шістдесят одна тисяча дев'ятсот шістдесят вісім — загальна модель.");
    check("Cyrillic technical identifiers", normalize_with("К145ІК512П; АТ1; О2; СО2; ТіО2; 38С2; Р-405м; БІО-100.", &audit_options), "ка сто сорок п'ять і ка п'ятсот дванадцять пе; а те один; о два; ес о два; те і о два; тридцять вісім ес два; ер дефіс чотириста п'ять ем; бе і о дефіс сто.");
    check("compound Cyrillic technical codes", normalize_with("Плита 1-0-1000х630; ВМ-23/25/27/32/1230.", &audit_options), "Плита один дефіс нуль дефіс тисяча помножити на шістсот тридцять; ве ем дефіс двадцять три слеш двадцять п'ять слеш двадцять сім слеш тридцять два слеш тисяча двісті тридцять.");
    check(
        "spaced Cyrillic dimensions",
        normalize_with("розмірами 1000 х 630 мм", &audit_options),
        "розмірами тисяча помножити на шістсот тридцять міліметрів",
    );
    check(
        "scientific notation without caret and with unit",
        normalize_with("1,76× 10-19 Дж", &audit_options),
        "одна ціла і сімдесят шість сотих помножити на десять у степені мінус дев'ятнадцять джоуля",
    );
    check(
        "named month consumes abbreviated year suffix",
        normalize_with("У липні 2011 р.", &audit_options),
        "У липні дві тисячі одинадцятого року",
    );
    check("zero ordinal", normalize_with("0-го класу", &audit_options), "нульового класу");
    check(
        "spaced abbreviation punctuation",
        normalize_with("і т.д .; Corp. створено", &audit_options),
        "і так далі; корп. створено",
    );
    let foreign_slash = normalize_with("Index locorum / Seznam krajev", &audit_options);
    check("foreign slash spacing", &foreign_slash, "індекс локорум/сезнам краджев");
    check(
        "foreign slash spacing is idempotent",
        normalize_with(&foreign_slash, &audit_options),
        &foreign_slash,
    );
    check(
        "abbreviation boundaries inside identifiers",
        normalize_with("ІЕР-01 і КР-005", &audit_options),
        "і е ер дефіс нуль один і ка ер дефіс нуль нуль п'ять",
    );
    check("spaced rate units", normalize_with("Швидкість 18 Мбіт / с. Затримка 160 мс; сигнал −116 дБм.", &audit_options), "Швидкість вісімнадцять мегабітів за секунду. Затримка сто шістдесят мілісекунд; сигнал мінус сто шістнадцять децибел-міліват.");
    check(
        "technical acronyms are not Roman numerals",
        normalize_with("Підфрейм DL, інтерфейс DVI та елемент III групи.", &audit_options),
        "Підфрейм ді ел, інтерфейс ді ві ай та елемент третьої групи.",
    );
    check(
        "dotted standard with letter suffix",
        normalize_with("Wi-Fi 6 (802.11ax)", &audit_options),
        "ві-фі шість (вісімсот два крапка одинадцять ей екс)",
    );
    check("bare IEEE revisions are identifiers, not numeric ranges", normalize_with("802.16-2005 (802.16e, 802.16m).", &audit_options), "вісімсот два крапка шістнадцять дефіс дві тисячі п'ять (вісімсот два крапка шістнадцять і, вісімсот два крапка шістнадцять ем).");
    check(
        "bare IEEE revision before sentence period",
        normalize_with("Стандарт 802.16m.", &audit_options),
        "Стандарт вісімсот два крапка шістнадцять ем.",
    );
    check(
        "bare IEEE revision with en dash",
        normalize_with("802.16–2005", &audit_options),
        "вісімсот два крапка шістнадцять дефіс дві тисячі п'ять",
    );
    check_no_category(
        "IEEE revision suffix is not an unknown unit",
        &flag_uncertain("802.16e"),
        UncertaintyCategory::Unit,
    );
    check_no_category(
        "IEEE revision suffix is not malformed scientific notation",
        &flag_uncertain("802.16e"),
        UncertaintyCategory::Scientific,
    );
    check(
        "classification code is not an invalid date",
        normalize_with("за спеціальністю 13.00.02", &audit_options),
        "за спеціальністю тринадцять крапка нуль нуль крапка нуль два",
    );
    check("dissertation speciality code after sciences label", normalize_with("Дисертація доктора технічних наук: 05.24.01 / університет.", &audit_options), "Дисертація доктора технічних наук: нуль п'ять крапка двадцять чотири крапка нуль один / університет.");
    check(
        "Wikipedia page citation metadata is not spoken",
        normalize_with(
            "Результат узгоджено з експериментом.:33–34:39–43 Так само виміряли густину.",
            &audit_options,
        ),
        "Результат узгоджено з експериментом. Так само виміряли густину.",
    );
    check(
        "speed-of-light variable is not a village abbreviation",
        normalize_with(
            "значення швидкості світла у вакуумі с. Перетворення статсіменса",
            &audit_options,
        ),
        "значення швидкості світла у вакуумі с. Перетворення статсіменса",
    );
    check(
        "numeric date consumes explicit year word",
        normalize_with("Подію завершили 25.06.1986 року.", &audit_options),
        "Подію завершили двадцять п'ятого червня тисяча дев'ятсот вісімдесят шостого року.",
    );
    check("explicit year span", normalize_with("З 1986 по 1991 рр. тривала програма.", &audit_options), "З тисяча дев'ятсот вісімдесят шостого до тисяча дев'ятсот дев'яносто першого року тривала програма.");
    check(
        "coordinate direction is not repeated",
        normalize_with("Точка лежить на 174°E довготи.", &audit_options),
        "Точка лежить на сто сімдесят чотири градуси східної довготи.",
    );
    check(
        "governed coordinate bounds",
        normalize_with("від 180° довготи до 174° W довготи", &audit_options),
        "від ста вісімдесяти градусів довготи до ста сімдесяти чотирьох градусів західної довготи",
    );
    let normalized_doi = normalize_with("doi:10.22059/jitm.2024.99052", &audit_options);
    check("DOI normalization", &normalized_doi, "ді оу ай десять крапка двадцять дві тисячі п'ятдесят дев'ять слеш джітм крапка дві тисячі двадцять чотири крапка дев'яносто дев'ять тисяч п'ятдесят два");
    check(
        "DOI normalization is idempotent",
        normalize_with(&normalized_doi, &audit_options),
        &normalized_doi,
    );
    check("bracketed IPv6 endpoint", normalize_with("[2001:db8::1]:443", &audit_options), "ай пі версії шість два нуль нуль один двокрапка ді бі вісім двокрапка скорочення нулів двокрапка один порт чотириста сорок три");
    check(
        "invalid IPv6 CIDR preserved",
        normalize_with("2001:db8::1/129", &audit_options),
        "2001:db8::1/129",
    );
    check(
        "balanced Markdown destination",
        normalize_with("[5 кг](https://example.com/a_(b)?x=1)", &audit_options),
        "[п'ять кілограмів](https://example.com/a_(b)?x=1)",
    );
    check("double backtick code", normalize_with("``x=`5` ``", &audit_options), "``x=`5` ``");
    check(
        "unicode hyphen temperature range",
        normalize_with("5‐7 °C", &audit_options),
        "від п'яти до семи градусів Цельсія",
    );
    check(
        "temperature range punctuation",
        normalize_with("5-7 °C.", &audit_options),
        "від п'яти до семи градусів Цельсія.",
    );
    check(
        "measurement terminal punctuation",
        normalize_with("Відстань становить 100 км.", &audit_options),
        "Відстань становить сто кілометрів.",
    );
    check(
        "governed abbreviated measurement with punctuation",
        normalize_with("Відстань становить до 2000 м.", &audit_options),
        "Відстань становить до двох тисяч метрів.",
    );
    check(
        "bibliographic page count",
        normalize_with("Монографія. — 279 с.: іл.", &audit_options),
        "Монографія. — двісті сімдесят дев'ять сторінок: іл.",
    );
    check(
        "bibliographic volume count",
        normalize_with("Енциклопедія: у 2 т. / ред. Іваненко.", &audit_options),
        "Енциклопедія: у двох томах / ред. Іваненко.",
    );
    check(
        "bibliographic singular volume",
        normalize_with("Довідник: в 1 т / ред. Іваненко.", &audit_options),
        "Довідник: в одному томі / ред. Іваненко.",
    );
    check(
        "single bibliographic page",
        normalize_with("Монографія. — С. 896.", &audit_options),
        "Монографія. — сторінка вісімсот дев'яносто шість.",
    );
    check(
        "mediawiki question heading",
        normalize_with("==== Чи може машина мислити? ====\nТекст відповіді.", &audit_options),
        "Чи може машина мислити?\nТекст відповіді.",
    );
    check(
        "compound measurement terminal punctuation",
        normalize_with("Швидкість становить 100 Мбіт/с.", &audit_options),
        "Швидкість становить сто мегабітів за секунду.",
    );
    check(
        "mixed-script data-rate denominator",
        normalize_with("Швидкість до 10 Гбіт/c.", &audit_options),
        "Швидкість до десяти гігабітів за секунду.",
    );
    check(
        "progressive video resolution after a quality label",
        normalize_with("Передача з 1080p-якістю.", &audit_options),
        "Передача з якістю тисяча вісімдесят пі.",
    );
    check(
        "bare progressive video resolution",
        normalize_with("Відео 720p.", &audit_options),
        "Відео сімсот двадцять пі.",
    );
    check(
        "capitalized kilobit unit",
        normalize_with("Швидкість становить 144 Кбіт/с.", &audit_options),
        "Швидкість становить сто сорок чотири кілобіти за секунду.",
    );
    check(
        "English tonne unit",
        normalize_with("Маса становить 30 tonnes.", &audit_options),
        "Маса становить тридцять тонн.",
    );
    check(
        "variable ratio",
        normalize_with("Розгалужувач має відношення 1:n.", &audit_options),
        "Розгалужувач має відношення один до ен.",
    );
    check(
        "locative number before adjective",
        normalize_with("Дані зберігають у 51 публічному домені.", &audit_options),
        "Дані зберігають у п'ятдесяти одному публічному домені.",
    );
    check(
        "mediawiki heading delimiters",
        normalize_with("== Історія ==\nПерший комп'ютер створили давно.", &audit_options),
        "Історія\nПерший комп'ютер створили давно.",
    );
    let technical_identifiers = normalize_with(
        "Протоколи IPv4 і IPv6 працюють у мережі 5G на x86; машини Z3 використовували RC4.",
        &audit_options,
    );
    check("technical alphanumeric identifiers", &technical_identifiers, "Протоколи ай пі версії чотири і ай пі версії шість працюють у мережі п'ять джі на ікс вісімдесят шість; машини зед три використовували ар сі чотири.");
    check(
        "technical alphanumeric identifiers are idempotent",
        normalize_with(&technical_identifiers, &audit_options),
        &technical_identifiers,
    );
    check(
        "single latin initial is stable",
        normalize_with(&normalize_with("Andrew S.", &audit_options), &audit_options),
        normalize_with("Andrew S.", &audit_options),
    );
    check("mixed vulgar fraction", normalize_with("2½", &audit_options), "дві цілих і одна друга");
    check(
        "measured mixed vulgar fraction",
        normalize_with("2½ кг", &audit_options),
        "дві цілих і одна друга кілограма",
    );
    check("measured fraction", normalize_with("3/4 кг", &audit_options), "три четвертих кілограма");
    check(
        "signed leading-dot measurement",
        normalize_with("-.5 кг", &audit_options),
        "мінус нуль цілих і п'ять десятих кілограма",
    );
    check(
        "temperature tolerance",
        normalize_with("5±0,2 °C", &audit_options),
        "п'ять плюс мінус нуль цілих і дві десятих градуса Цельсія",
    );
    check(
        "bare Celsius range",
        normalize_with("5-7 C", &audit_options),
        "від п'яти до семи градусів Цельсія",
    );
    check("midnight AM", normalize_with("12:00 AM", &audit_options), "опівночі");
    let mut short_clock_options = audit_options.clone();
    short_clock_options.colon_style = ColonStyle::Clock;
    check(
        "short-minute clock",
        normalize_with("10:5", &short_clock_options),
        "десять годин п'ять хвилин",
    );
    check(
        "invalid timezone preserved",
        normalize_with("10:30 UTC+14:30", &audit_options),
        "10:30 UTC+14:30",
    );
    check(
        "invalid calendar date preserved",
        normalize_with("29.02.2023", &audit_options),
        "29.02.2023",
    );
    check(
        "out-of-range geo URI preserved",
        normalize_with("geo:90.0001,180", &audit_options),
        "geo:90.0001,180",
    );
    check(
        "coordinate beats Newton symbol",
        normalize_with("3°N", &audit_options),
        "три градуси північної широти",
    );
    check("spaced Newton symbol", normalize_with("3 °N", &audit_options), "три градуси Ньютона");
    check(
        "legal article words",
        normalize_with("статті 5—7", &audit_options),
        "від п'ятої до сьомої статті",
    );
    check(
        "basis points not address",
        normalize_with("10 б.п.", &audit_options),
        "десять базисних пунктів",
    );
    check(
        "grouped symbol currency",
        normalize_with("$1,234.56", &audit_options),
        "тисяча двісті тридцять чотири долари п'ятдесят шість центів",
    );
    check("regional currency", normalize_with("CA$5", &audit_options), "п'ять канадських доларів");
    check(
        "case-insensitive regional currency",
        normalize_with("ca$5", &audit_options),
        "п'ять канадських доларів",
    );
    check(
        "single grouped currency",
        normalize_with("$1,234", &audit_options),
        "тисяча двісті тридцять чотири долари",
    );
    check("accounting currency", normalize_with("($5)", &audit_options), "мінус п'ять доларів");
    check("ISO currency", normalize_with("5 AED", &audit_options), "п'ять дирхамів ОАЕ");
    check(
        "three-digit currency minor unit",
        normalize_with("1.234 BHD", &audit_options),
        "один бахрейнський динар двісті тридцять чотири філси",
    );
    check(
        "four-digit currency minor unit",
        normalize_with("1.2345 CLF", &audit_options),
        "одна чилійська розрахункова одиниця дві тисячі триста сорок п'ять десятитисячних частин",
    );
    check(
        "zero-digit currency decimal",
        normalize_with("1.5 JPY", &audit_options),
        "одна ціла і п'ять десятих єн",
    );
    check("fiat pair", normalize_with("AED/USD", &audit_options), "дирхамів ОАЕ до доларів США");
    check("named cryptocurrency", normalize_with("2 AVAX", &audit_options), "два аваланчі");
    check(
        "lowercase named cryptocurrency",
        normalize_with("2 avax", &audit_options),
        "два аваланчі",
    );
    check("prefixed cryptocurrency", normalize_with("BTC 2", &audit_options), "два біткоїни");
    check(
        "grouped cryptocurrency",
        normalize_with("1,000 BTC", &audit_options),
        "тисяча біткоїнів",
    );
    check(
        "localized grouped cryptocurrency",
        normalize_with("1.000,25 ETH", &audit_options),
        "тисяча цілих і двадцять п'ять сотих ефіра",
    );
    check(
        "bitcoin symbol prefix",
        normalize_with("₿0.5", &audit_options),
        "нуль цілих і п'ять десятих біткоїна",
    );
    check(
        "bitcoin symbol suffix",
        normalize_with("0,5 ₿", &audit_options),
        "нуль цілих і п'ять десятих біткоїна",
    );
    check(
        "generic cryptocurrency ticker",
        normalize_with("0.25 NEWCOIN", &audit_options),
        "нуль цілих і двадцять п'ять сотих ен і дабл ю сі оу ай ен",
    );
    check(
        "generic cryptocurrency pair",
        normalize_with("NEWCOIN/USDT", &audit_options),
        "ен і дабл ю сі оу ай ен до тезерів",
    );
    check(
        "technical slash acronyms are not finance pairs",
        normalize_with("Протоколи TCP/IP та IPX/SPX.", &audit_options),
        "Протоколи ті сі пі слеш ай пі та ай пі екс слеш ес пі екс.",
    );
    check("technical acronym numbers keep their order", normalize_with("Стандарти ISO 3166 та IEEE 802.3; мова ALGOL 58.", &audit_options), "Стандарти ай ес оу три тисячі сто шістдесят шість та ай і і і вісімсот два крапка три; мова ей ел джі оу ел п'ятдесят вісім.");
    check(
        "lowercase known cryptocurrency pair",
        normalize_with("btc/eth", &audit_options),
        "біткоїнів до ефірів",
    );
    let iso_codes = "AFN EUR ALL DZD USD AOA XCD XAD ARS AMD AWG AUD AZN BSD BHD BDT BBD BYN BZD XOF BMD INR BTN BOB BOV BAM BWP NOK BRL BND BIF CVE KHR XAF CAD KYD CLP CLF CNY COP COU KMF CDF NZD CRC CUP XCG CZK DKK DJF DOP EGP SVC ERN SZL ETB FKP FJD XPF GMD GEL GHS GIP GTQ GBP GNF GYD HTG HNL HKD HUF ISK IDR XDR IRR IQD ILS JMD JPY JOD KZT KES KPW KRW KWD KGS LAK LBP LSL ZAR LRD LYD CHF MOP MKD MGA MWK MYR MVR MRU MUR XUA MXN MXV MDL MNT MAD MZN MMK NAD NPR NIO NGN OMR PKR PAB PGK PYG PEN PHP PLN QAR RON RUB RWF SHP WST STN SAR RSD SCR SLE SGD XSU SBD SOS SSP LKR SDG SRD SEK CHE CHW SYP TWD TJS TZS THB TOP TTD TND TRY TMT UGX UAH AED USN UYU UYI UYW UZS VUV VES VED VND YER ZMW ZWG XBA XBB XBC XBD XTS XXX XAU XPD XPT XAG";
    for code in iso_codes.split_ascii_whitespace() {
        check_absent(
            &format!("ISO 4217 coverage {code}"),
            normalize_with(&format!("2 {code}"), &audit_options),
            code,
        );
    }
    check(
        "invalid bracketed IPv6 port preserved",
        normalize_with("[2001:db8::1]:65536", &audit_options),
        "[2001:db8::1]:65536",
    );
    check(
        "invalid bare timezone preserved",
        normalize_with("10:30 +14:01", &audit_options),
        "10:30 +14:01",
    );
    check("Cisco MAC", normalize_with("aabb.ccdd.eeff", &audit_options), "мак адреса ей ей двокрапка бі бі двокрапка сі сі двокрапка ді ді двокрапка і і двокрапка еф еф");
}
