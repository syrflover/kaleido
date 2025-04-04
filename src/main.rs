#![allow(clippy::collapsible_else_if)]
use std::{fs::File, io::BufReader};

use arrayvec::ArrayVec;
use byteview::ByteView;
use futures::{StreamExt, stream};
use rkiwi::{Kiwi, KiwiBuilder, Match, POSTag, analyzed::Token};
use tokio::fs;
use widestring::{U16Str, U16String, u16str};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    let mut reader = BufReader::new(File::open("data.json")?);

    let bytes =
        ByteView::from_reader(&mut reader, fs::metadata("data.json").await?.len() as usize)?;

    let xs = serde_json::from_slice::<Vec<String>>(&bytes)?;

    stream::iter(xs)
        // .take()
        .for_each_concurrent(6, |x| {
            let kiwi = kiwi.clone();
            async move {
                process(&kiwi, &x).unwrap();
            }
        })
        .await;

    // let mut index = index_factory(8, "Flat", MetricType::L2).unwrap();

    // index.add(x);

    Ok(())
}

fn is_korean(pos_tag: &POSTag) -> bool {
    !(POSTag::SF..=POSTag::W_EMOJI).contains(pos_tag)
}

fn is_special(pos_tag: &POSTag) -> bool {
    (POSTag::SF..=POSTag::SB).contains(pos_tag) || (POSTag::SN..=POSTag::W_EMOJI).contains(pos_tag)
}

fn find_subtitle<'a>(
    xs: impl Iterator<Item = &'a (U16String, Token)>,
) -> ArrayVec<(usize, usize), 2> {
    let mut res = ArrayVec::<_, 2>::new();

    let mut start = 0;
    let mut open_so = false;

    for (i, (_form, token)) in xs.enumerate() {
        if token.tag == POSTag::SO && !open_so {
            open_so = true;
            start = i;
        } else if token.tag == POSTag::SO && open_so {
            open_so = false;
            if res.try_push((start, i)).is_err() {
                return res;
            }
        }
    }

    res
}

#[test]
fn test_find_subtitle() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    let txt = U16String::from_str(
        "비밀의 버스 투어 ~나의 버스 가이드 일지~ [korean} Himitsu no Bus Tour ~Boku no Bus Guide Nisshi~",
    );
    let match_options = Match::new().all_with_normailize_coda();
    let analyzed = kiwi.analyze_w(&txt, 1, match_options, None, None)?;
    let xs = analyzed.to_vec_w();
    let res = find_subtitle(xs.iter());

    fn start(xs: &[(U16String, Token)], res: &[(usize, usize)], i: usize) -> usize {
        let token = xs[res[i].0].1;
        token.chr_position
    }

    fn end(xs: &[(U16String, Token)], res: &[(usize, usize)], i: usize) -> usize {
        let token = xs[res[i].1].1;
        token.chr_position + token.length
    }

    assert_eq!(
        &txt[start(&xs, &res, 0)..end(&xs, &res, 0)].to_string()?,
        "~나의 버스 가이드 일지~"
    );
    assert_eq!(
        &txt[start(&xs, &res, 1)..end(&xs, &res, 1)].to_string()?,
        "~Boku no Bus Guide Nisshi~"
    );

    Ok(())
}

fn find_korean<'a>(
    xs: impl Iterator<Item = &'a (U16String, Token)> + Clone,
    reversed: bool,
) -> Option<usize> {
    // let has_sso_ssc = {
    //     let sso = xs.clone().position(|(_, t)| t.tag == POSTag::SSO);
    //     let ssc = xs.clone().position(|(_, t)| t.tag == POSTag::SSC);

    //     matches!(sso.zip(ssc), Some((sso, ssc)) if sso < ssc)
    // };

    let subtitles = find_subtitle(xs.clone());

    let mut iter = xs.enumerate().peekable();

    let mut open_ss = false;
    let mut open_so = false;
    let mut subtitle_count = 0;
    let mut start_episode = false;

    loop {
        let (curr_i, (_curr_form, curr_token)) = iter.next()?;

        if open_so {
            subtitle_count += 1;
        }

        if is_korean(&curr_token.tag) {
            if let Some((next_i, (_next_form, next_token))) = iter.peek() {
                if is_special(&next_token.tag) && next_token.tag != POSTag::SP && !open_ss {
                    if next_token.tag == POSTag::SN {
                        return Some(curr_i);
                    } else {
                        return Some(*next_i);
                    }
                } else {
                    return Some(curr_i);
                }
            }
        } else {
            if is_special(&curr_token.tag) && curr_token.tag != POSTag::SP && !open_ss {
                if let Some((next_i, (_next_form, next_token))) = iter.peek() {
                    // 역방향인 경우에 일부 보정이 필요함
                    //
                    // 정방향일 경우에는 한국어가 뒤에 있기 때문에 보정이 필요 없지만
                    // 역방향일 경우에는 외국어를 먼저 마주하기 때문에 보정이 필요함
                    if reversed {
                        // 역방향일 경우, range episode가 끝까지 추출되지 않는 문제를 해결함
                        if start_episode
                            && curr_token.tag == POSTag::SO
                            && next_token.tag == POSTag::SN
                        {
                            return Some(*next_i - 2);
                        } else {
                            start_episode = false;
                        }

                        // 역방향일 경우, 이른 subtitle 추출을 방지함
                        // 해당 조건이 없으면 한국어 제목과 부제목이 모두 날아가고, 외국어만 추출됨
                        if curr_token.tag == POSTag::SO && subtitle_count > 0 {
                            match subtitles.get(1) {
                                Some((_s, e)) if *e != curr_i => {}
                                _ => {
                                    return Some(curr_i - subtitle_count);
                                }
                            }
                        }
                    }

                    if is_korean(&next_token.tag) && !open_so {
                        if !reversed
                            && (curr_token.tag == POSTag::SF || curr_token.tag == POSTag::SW)
                        {
                        } else {
                            return Some(curr_i);
                        }
                    }
                }
            }

            // TODO: 여는 부호와 닫는 부호가 둘 다 있는지 체크해야함?
            if curr_token.tag == POSTag::SSO || curr_token.tag == POSTag::SSC {
                open_ss = !open_ss;
            }

            if curr_token.tag == POSTag::SO {
                // 부제목 토큰 개수 초기화
                if open_so && subtitle_count > 0 {
                    subtitle_count = 0;
                }

                open_so = !open_so;
            }

            if !start_episode && curr_token.tag == POSTag::SN {
                start_episode = true;
            }
        }
    }
}

const PIPE_CHARS: [&U16Str; 4] = [u16str!("│"), u16str!("|"), u16str!("｜"), u16str!("ㅣ")];

fn is_pipe(x: impl AsRef<U16Str>) -> bool {
    PIPE_CHARS.contains(&x.as_ref())
}

fn process(kiwi: &Kiwi, text: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let text = U16String::from_str(text);
    let text = text.as_ustr();

    let match_options = Match::new()
        // .split_saisiot(true)
        // .compatible_jamo(true)
        .all_with_normailize_coda();

    let analyzed = kiwi.analyze_w(text, 1, match_options, None, None)?;

    let xs = analyzed.to_vec_w();

    for (form, token) in &xs {
        print!("\"{}\" {} / ", form.display(), token.tag);
    }
    println!();

    let mut reverse = false;

    // count(S..=W)
    let mut fr_count = 0;
    let mut has_ko = false;
    let mut has_pipe = false;

    let mut ko_start = 0;
    let first_korean = find_korean(xs.iter(), false);

    for (i, (form, token)) in xs.iter().enumerate() {
        if is_korean(&token.tag) || has_ko {
            fr_count = 0;
            has_ko = true;
        } else {
            fr_count += 1;
        }

        if is_pipe(form) {
            if has_pipe {
                continue;
            }

            // 해당 반복문은 `foreign | 한글` 에서 한글을 추출함
            // pipe를 만나기도 전에 한글이 있으면 반대로 돌아야함
            if has_ko {
                fr_count = 0;
                reverse = true;
                break;
            }

            has_pipe = true;
            fr_count -= 1;

            let score = fr_count as f32 / i as f32;

            // println!("score   : {:.5} / {}", score, form.display());

            if score >= 1.0 {
                ko_start = token.chr_position + token.length;
            } else {
                break;
            }
        }

        match first_korean {
            Some(first_korean) if !has_pipe && i < first_korean => {
                if has_ko {
                    fr_count = 0;
                    reverse = true;
                    break;
                }

                let score = fr_count as f32 / i as f32;

                // println!("score   : {:.5} / {}", score, form.display());

                if score >= 1.0 {
                    ko_start = token.chr_position + token.length;
                } else {
                    break;
                }
            }
            Some(0) => {
                if has_ko {
                    fr_count = 0;
                    reverse = true;
                    break;
                }
            }
            _ => {}
        }
    }

    let mut ko_end = 0;

    if reverse {
        let last_korean = find_korean(xs.iter().rev(), true).map(|pos| xs.len() - pos);

        let mut ko_end_index = 0;
        ko_end = {
            let last = xs.last().unwrap().1;
            last.chr_position + last.length
        };
        for (i, (form, token)) in xs.iter().enumerate() {
            if is_korean(&token.tag) || has_ko {
                fr_count = 0;
                has_ko = true;
            } else {
                fr_count += 1;
            }

            if is_pipe(form) {
                if has_pipe {
                    continue;
                }

                has_pipe = true;
                fr_count -= 1;

                ko_end = token.chr_position;
                ko_end_index = i;
            }

            match last_korean {
                Some(last_korean) if !has_pipe && i < last_korean => {
                    let score = fr_count as f32 / i as f32;

                    // println!("score   : {:.5} / {}", score, form.display());

                    if score <= 0.0 {
                        ko_end = token.chr_position + token.length;
                        ko_end_index = i;
                    }
                }
                _ => {}
            }
        }

        let score = (xs.len() - ko_end_index) as f32 / fr_count as f32;

        // println!("score   : {:.5}", score,);

        if score < 1.0 {
            ko_end = 0;
        }
    }

    println!("reverse : {}", reverse);

    println!("origin  : {}", text.display());
    let res = if reverse {
        if has_ko {
            let fr_start = ko_end;
            let fr = text[fr_start..].to_string().unwrap().trim().to_owned();
            let ko = text[..ko_end].to_string().unwrap().trim().to_owned();

            if !fr.is_empty() {
                println!("foreign : {}", fr);
            }
            println!("korean  : {}", ko);

            (fr, ko)
        } else {
            println!("foreign : {}", text.display());
            (text.to_string().unwrap(), String::new())
        }
    } else if has_ko {
        let fr_end = if has_pipe { ko_start - 1 } else { ko_start };
        let fr = text[..fr_end].to_string().unwrap().trim().to_owned();
        let ko = text[ko_start..].to_string().unwrap().trim().to_owned();

        if !fr.is_empty() {
            println!("foreign : {}", fr);
        }
        println!("korean  : {}", ko);

        (fr, ko)
    } else {
        println!("foreign : {}", text.display());
        (text.to_string().unwrap(), String::new())
    };

    println!("----------------------------");

    Ok(res)
}

// TODO: 쓸데없는 문자 제거
// 토끼 구멍에 빠지다 (Blue Archive) [Korean} <- [Korean}
// 슈텐도지 (decensored) <- (decensored) / 제거하기 전에 작품 정보에 검열되지 않았다는 것을 표기해야함 (uncensored)
// 울보 공주와 사천왕 시오후키 섹스 4번 승부 [Korean]
//
// TODO: 외국어 제목에만 에피소드 숫자가 있는 경우
// foreign : Haha to Ochite Iku Part 2
// korean  : 엄마와 함께 타락해 간다
//
// TODO: 한국어 제목에만 에피소드 숫자가 있는 경우
// foreign : Pokemon SV MTR
// korean  : 포켓몬 SV MTR 6-7
//
// TODO: 에피소드는 아니지만
// foreign : Mama Mansion! Dainiwa 601 Goushitsu Sonosaki Kaoru (33)
// korean  : 마마 맨션! 제2화 601호실 소노자키 카오루
//
// origin  : 흰여울 _ Huin_Yeou
// foreign : Huin_Yeou
// korean  : 흰여울 _
//
// origin  : Shaving Archive -Sukitoru Yona Sekaikan Nanoni Vol.05- | 셰이빙 아카이브
// foreign : Shaving Archive -Sukitoru Yona Sekaikan Nanoni Vol.05-
// korean  : 셰이빙 아카이브
//
// origin  : Zemi no Bounenkai (Zenpen) | 세미나 송년회 (decensored)
// foreign : Zemi no Bounenkai (Zenpen)
// korean  : 세미나 송년회 (decensored)
//
// "Miru" SL / "Harumin" SL / "!" SF / "To" SL / "Harent" SL / "#2" W_HASHTAG / "|" SW / "투하트" NNP /
// reverse : false
// origin  : Miru Harumin! To Harent#2 | 투하트
// foreign : Miru Harumin! To Harent#2
// korean  : 투하트
//
// 에피소드가 한국어에만 있는 건 상관 없음
// 외국어에만 있고, 한국어에만 없는 걸 찾아내서 가져와야함
// 규칙을 새로 만들기: vol vol. ch ch. part 뒤에 숫자가 있는 경우 또는 (Zenpen) 같은 것들 => H_EP
//
// ch.1 Vol.05 (Zenpen) 전편
//
// 이미지셋은 지원하지 않는 걸로 하자. 이런 거 너무 많음
// origin  : KissNTR.Gold 3.일러모음
// foreign : KissNTR.Gold
// korean  : 3.일러모음
//
// 이건 이미지셋 아님.
// "철" NNG / "혈" NNG / "M" SL / "16" SN /
// reverse : true
// origin  : 철혈 M16
// foreign : M16
// korean  : 철혈
//
// origin  : 네게브 x 카98k / Negev x Kar98k
// foreign : k / Negev x Kar98k
// korean  : 네게브 x 카98
//
// reverse : true
// origin  : Yuukyuu no Shou Elf 1 "Dokuhebi" | 유구의 창엘프 1 "독사"
// foreign : | 유구의 창엘프 1 "독사"
// korean  : Yuukyuu no Shou Elf 1 "Dokuhebi"
//
// reverse : true
// origin  : Nakama de Pokapoka+후일담추가 | 따끈따끈 친구
// foreign : | 따끈따끈 친구
// korean  : Nakama de Pokapoka+후일담추가
//
// reverse : true
// origin  : Yuukyuu no Shou Elf 2 "Shoukei" | 유구의 창엘프 2 "동경"
// foreign : | 유구의 창엘프 2 "동경"
// korean  : Yuukyuu no Shou Elf 2 "Shoukei"
//
// reverse : true
// origin  : 설이벗방TV♥
// foreign : TV♥
// korean  : 설이벗방
//
// reverse : true
// origin  : 헬스녀 개인PT♥
// foreign : PT♥
// korean  : 헬스녀 개인
//
// reverse : true
// origin  : 도구의 올바른 사용법 Vol.3
// foreign : Vol.3
// korean  : 도구의 올바른 사용법
//
// reverse : true
// origin  : Hamechichi! 하메찌찌! Ch. 1 (uncensored)
// foreign : Ch. 1 (uncensored)
// korean  : Hamechichi! 하메찌찌!
//
// reverse : true
// origin  : Ano! Okaa-san no Shousai - Manatsu no Oyako SEX l 그! 엄마의 상세 ~한여름의 모자 SEX~
// foreign : SEX~
// korean  : Ano! Okaa-san no Shousai - Manatsu no Oyako SEX l 그! 엄마의 상세 ~한여름의 모자
//
// 이런 식으로 같은 제목임에도 한국어 제목이 특정 작품에만 없는 경우, 한국어 제목을 생성해줘야함
// "Expert ni Narimashita! 5 | 전문가가 되었습니다! 5",
// "Expert ni Narimashita! 4",
// "Expert ni Narimashita! 3 | 전문가가 되었습니다! 3",
//
// "Ja Ja Ja Ja Japan | 재재재재 재빵 1",
// "Ja Ja Ja Ja Japan 2",
// "Ja Ja Ja Ja Japan 3",

#[test]
fn foreign_only() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    let txt = "AZA!!🔞";
    let res = process(&kiwi, txt)?;
    assert_eq!(res.0, txt);
    assert!(res.1.is_empty());

    let txt = "Patreon 2019/02~2025/02 Tier2 Reward";
    let res = process(&kiwi, txt)?;
    assert_eq!(res.0, txt);
    assert!(res.1.is_empty());

    let txt = "Senko & Shiro X Horse | Senko & Shiro X Horse";
    let res = process(&kiwi, txt)?;
    assert_eq!(res.0, txt);
    assert!(res.1.is_empty());

    Ok(())
}

#[test]
fn range_episode() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    let txt = "미유 쨩이 선생님의 육단지 오나펫이 되는 이야기 1~13 Miyu-chan ga Sensei no Nikutsubo Onapet ni Naru Hanashi";
    let res = process(&kiwi, txt)?;
    assert_eq!(
        res.0,
        "Miyu-chan ga Sensei no Nikutsubo Onapet ni Naru Hanashi"
    );
    assert_eq!(res.1, "미유 쨩이 선생님의 육단지 오나펫이 되는 이야기 1~13");

    let txt = "Miyu-chan ga Sensei no Nikutsubo Onapet ni Naru Hanashi 미유 쨩이 선생님의 육단지 오나펫이 되는 이야기 1~24 ";
    let res = process(&kiwi, txt)?;
    assert_eq!(
        res.0,
        "Miyu-chan ga Sensei no Nikutsubo Onapet ni Naru Hanashi"
    );
    assert_eq!(res.1, "미유 쨩이 선생님의 육단지 오나펫이 되는 이야기 1~24");

    Ok(())
}

#[test]
fn korean_only_with_subtitle() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    let txt = "그녀들의 말할 수 없는 비밀 - White Lie & Dark Truth -";
    let res = process(&kiwi, txt)?;
    assert!(res.0.is_empty());
    assert_eq!(
        res.1,
        "그녀들의 말할 수 없는 비밀 - White Lie & Dark Truth -"
    );

    Ok(())
}

#[test]
fn foreign_ends_with_special() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    let txt = "Onee-chan ni Sennou Sarechau! 누나에게 세뇌당해 버려!";
    let res = process(&kiwi, txt)?;
    assert_eq!(res.0, "Onee-chan ni Sennou Sarechau!");
    assert_eq!(res.1, "누나에게 세뇌당해 버려!");

    let txt =
        "Dekachin Sokuochi Gal Succubus + W Succubus to Houkago H + 지뢰계 서큐버스의 변태 간병";
    let res = process(&kiwi, txt)?;
    assert_eq!(
        res.0,
        "Dekachin Sokuochi Gal Succubus + W Succubus to Houkago H +"
    );
    assert_eq!(res.1, "지뢰계 서큐버스의 변태 간병");

    Ok(())
}

#[test]
fn korean_only() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    let txt = "봇치님의 변태여친 1";
    let res = process(&kiwi, txt)?;
    assert_eq!(res.1, txt);
    assert!(res.0.is_empty());

    // FIXME:
    // let txt = "풍기위원 쿠로이와 리호코의 경우 | 풍기위원 쿠로이와 리호코의 경우";
    // let res = process(&kiwi, txt)?;
    // assert_eq!(txt, res.1);
    // assert!(res.0.is_empty());

    Ok(())
}

#[test]
fn korean_only_with_sw() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    // "애액" NNP / "스노우" NNP / "볼" NNG / "🎄" SW /
    let txt = "애액 스노우볼🎄";
    let res = process(&kiwi, txt)?;
    assert_eq!(res.1, txt);
    assert!(res.0.is_empty());

    Ok(())
}

#[test]
fn open_and_close_ss() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    // normal
    let txt = "Himitsu no Bus Tour ~Boku no Bus Guide Nisshi~ [korean] 비밀의 버스 투어 ~나의 버스 가이드 일지~";
    let res = process(&kiwi, txt)?;
    assert_eq!(
        res.0,
        "Himitsu no Bus Tour ~Boku no Bus Guide Nisshi~ [korean]"
    );
    assert_eq!(res.1, "비밀의 버스 투어 ~나의 버스 가이드 일지~");

    // reverse
    let txt = "비밀의 버스 투어 ~나의 버스 가이드 일지~ [korean} Himitsu no Bus Tour ~Boku no Bus Guide Nisshi~";
    let res = process(&kiwi, txt)?;
    assert_eq!(
        res.0,
        "[korean} Himitsu no Bus Tour ~Boku no Bus Guide Nisshi~"
    );
    assert_eq!(res.1, "비밀의 버스 투어 ~나의 버스 가이드 일지~");

    Ok(())
}

#[test]
fn normal() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    let txt = "Gakuen IDOLM@STER Fundoshi Goudou | 학원 아이돌마스터 훈도시 합동";
    let res = process(&kiwi, txt)?;
    assert_eq!(res.0, "Gakuen IDOLM@STER Fundoshi Goudou");
    assert_eq!(res.1, "학원 아이돌마스터 훈도시 합동");

    Ok(())
}

#[test]
fn reverse() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    // hasn't pipe
    let txt = "남친 있는 백인 코스어, 공개 엉덩이 구멍 확장 방송 / Kareshi Mochi Hakujin Layer, Koukai Ketsuana Kakuchou Haishin";
    let res = process(&kiwi, txt)?;
    assert_eq!(
        res.0,
        "/ Kareshi Mochi Hakujin Layer, Koukai Ketsuana Kakuchou Haishin"
    );
    assert_eq!(res.1, "남친 있는 백인 코스어, 공개 엉덩이 구멍 확장 방송");

    // has pipe
    let txt = "미소녀 전사 세일러 문 -유성에서 온 번식자-｜Bishoujo Senshi Sailor Moon Yuusei kara no Hanshoku-sha";
    let res = process(&kiwi, txt)?;
    assert_eq!(
        res.0,
        "｜Bishoujo Senshi Sailor Moon Yuusei kara no Hanshoku-sha"
    );
    assert_eq!(res.1, "미소녀 전사 세일러 문 -유성에서 온 번식자-");

    let txt = "있을 곳이 없어 카미마치 해본 버려진 소년의 에로망가 제2화 Ibasho ga Nai node Kamimachi shite mita Suterareta Shounen no Ero Manga 2";
    let res = process(&kiwi, txt)?;
    assert_eq!(
        res.0,
        "Ibasho ga Nai node Kamimachi shite mita Suterareta Shounen no Ero Manga 2"
    );
    assert_eq!(
        res.1,
        "있을 곳이 없어 카미마치 해본 버려진 소년의 에로망가 제2화"
    );

    Ok(())
}
